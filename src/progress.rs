#[cfg(test)]
use crate::domain::MODEL_ID;
use crate::domain::{Emit, Event};
use std::io::{self, Read};

pub fn build_steps(editions: usize, model: &str) -> u32 {
    9 + 2 * editions as u32
        + u32::from(crate::setup_recipe::SetupRecipe::for_model(model).is_some())
}

#[derive(Default)]
pub struct BuildStatus {
    pub completed: u32,
    pub total: u32,
    pub operation: Option<(String, Option<f32>)>,
    terminal: bool,
}
impl BuildStatus {
    pub fn apply(&mut self, event: &Event) {
        if self.terminal {
            return;
        }
        match event {
            Event::OverallProgress { completed, total } if *total > 0 => {
                let completed = (*completed).min(total - 1);
                self.completed = if self.total == *total {
                    self.completed.max(completed)
                } else {
                    completed
                };
                self.total = *total;
            }
            Event::OperationProgress { operation, percent } => {
                self.operation = (!operation.is_empty()).then(|| {
                    (
                        operation.clone(),
                        percent
                            .filter(|p| p.is_finite())
                            .map(|p| p.clamp(0.0, 100.0)),
                    )
                });
            }
            Event::Stage { .. } => self.operation = None,
            Event::Finished(_) => {
                self.completed = self.total;
                self.operation = None;
                self.terminal = true;
            }
            Event::Failed(_) | Event::Cancelled => {
                self.operation = None;
                self.terminal = true;
            }
            _ => (),
        }
    }
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| self.completed as f32 / self.total as f32)
    }
    pub fn label(&self) -> String {
        if self.total == 0 {
            "Preparing build…".into()
        } else if self.completed == self.total {
            format!("{} of {} steps complete", self.total, self.total)
        } else {
            format!("Step {} of {}", self.completed + 1, self.total)
        }
    }
}

pub struct BuildSteps {
    completed: u32,
    total: u32,
}
impl BuildSteps {
    pub fn new(editions: usize, model: &str, completed: u32, emit: &Emit) -> Self {
        let result = Self {
            completed,
            total: build_steps(editions, model),
        };
        result.emit(emit);
        result
    }
    pub fn advance(&mut self, emit: &Emit) {
        // Only Finished may display 100%, after history and worker cleanup finish.
        self.completed = (self.completed + 1).min(self.total - 1);
        self.emit(emit);
    }
    fn emit(&self, emit: &Emit) {
        emit(Event::OverallProgress {
            completed: self.completed,
            total: self.total,
        });
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Tool {
    Dism,
    Oscdimg,
}
impl Tool {
    pub fn operation(self, args: &[String]) -> String {
        match self {
            Self::Oscdimg => "Creating ISO",
            Self::Dism if args.iter().any(|a| a.eq_ignore_ascii_case("/Export-Image")) => {
                "Exporting Windows edition"
            }
            Self::Dism if args.iter().any(|a| a.eq_ignore_ascii_case("/Mount-Image")) => {
                "Mounting Windows image"
            }
            Self::Dism if args.iter().any(|a| a.eq_ignore_ascii_case("/Commit")) => {
                "Saving Windows image"
            }
            Self::Dism if args.iter().any(|a| a.eq_ignore_ascii_case("/Discard")) => {
                "Discarding Windows image"
            }
            Self::Dism if args.iter().any(|a| a.eq_ignore_ascii_case("/Add-Driver")) => {
                "Importing drivers"
            }
            Self::Dism => "Servicing Windows image",
        }
        .into()
    }
    fn percent(self, text: &str) -> Option<f32> {
        let value = match self {
            Self::Dism => {
                let bar = text.strip_prefix('[')?.strip_suffix(']')?;
                if !bar
                    .bytes()
                    .all(|c| c.is_ascii_digit() || b" =.%".contains(&c))
                {
                    return None;
                }
                let (left, right) = bar.split_once('%')?;
                if !right.bytes().all(|c| b" =".contains(&c)) {
                    return None;
                }
                left.trim_start_matches([' ', '=']).trim()
            }
            Self::Oscdimg => text
                .trim_end_matches('.')
                .strip_suffix("% complete")?
                .trim(),
        };
        let percent: f32 = value.parse().ok()?;
        (percent.is_finite() && (0.0..=100.0).contains(&percent)).then_some(percent)
    }
}

/// Commands report progress using CR as well as LF. Decode each complete update
/// as it arrives, retaining its text in the log independently of progress events.
pub(crate) fn read_output(
    mut input: impl Read,
    tool: Option<Tool>,
    operation: &str,
    emit: &Emit,
) -> io::Result<()> {
    let mut buffer = [0; 4096];
    let mut pending = Vec::new();
    let flush = |pending: &mut Vec<u8>| {
        let text = String::from_utf8_lossy(pending).replace('\0', "");
        let text = text.trim();
        if !text.is_empty() {
            if let Some(percent) = tool.and_then(|tool| tool.percent(text)) {
                emit(Event::OperationProgress {
                    operation: operation.into(),
                    percent: Some(percent),
                });
            }
            emit(Event::Log(text.into()));
        }
        pending.clear();
    };
    loop {
        let size = input.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        for byte in &buffer[..size] {
            if *byte == b'\r' || *byte == b'\n' {
                flush(&mut pending);
            } else {
                pending.push(*byte);
            }
        }
    }
    flush(&mut pending);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[test]
    fn operation_resets_independently_and_terminal_states_preserve_overall() {
        let mut status = BuildStatus::default();
        status.apply(&Event::OverallProgress {
            completed: 4,
            total: 12,
        });
        status.apply(&Event::OperationProgress {
            operation: "Exporting".into(),
            percent: Some(100.0),
        });
        assert_eq!(status.fraction(), Some(4.0 / 12.0));
        status.apply(&Event::Stage {
            name: "Next".into(),
            detail: String::new(),
        });
        assert!(status.operation.is_none());
        status.apply(&Event::OperationProgress {
            operation: "Saving".into(),
            percent: None,
        });
        assert!(status.operation.as_ref().unwrap().1.is_none());
        status.apply(&Event::Cancelled);
        assert!(status.operation.is_none());
        status.apply(&Event::OverallProgress {
            completed: 11,
            total: 12,
        });
        assert_eq!(status.completed, 4);
        let mut done = BuildStatus::default();
        done.apply(&Event::OverallProgress {
            completed: 12,
            total: 12,
        });
        assert!(done.fraction().unwrap() < 1.0);
        done.apply(&Event::Finished("output.iso".into()));
        assert_eq!(done.fraction(), Some(1.0));
        assert_eq!(done.label(), "12 of 12 steps complete");
    }
    #[test]
    fn parses_only_supported_tool_progress() {
        assert_eq!(Tool::Dism.percent("[==== 42.5%==== ]"), Some(42.5));
        assert_eq!(Tool::Oscdimg.percent("99% complete."), Some(99.0));
        for text in [
            "100% available",
            "[foo 20%]",
            "[ 101% ]",
            "[-1%]",
            "[NaN%]",
            "[1% 2%]",
        ] {
            assert_eq!(Tool::Dism.percent(text), None);
            assert_eq!(Tool::Oscdimg.percent(text), None);
        }
    }
    #[test]
    fn fragmented_crlf_and_utf16_updates_retain_logs() {
        struct Fragmented(std::io::Cursor<Vec<u8>>);
        impl Read for Fragmented {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                self.0.read(&mut out[..1])
            }
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let copy = events.clone();
        let emit: Emit = Arc::new(move |e| copy.lock().unwrap().push(e));
        let raw = "[= 5.0% ]\r[== 12.5% ]\r\nDone"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        read_output(
            Fragmented(std::io::Cursor::new(raw)),
            Some(Tool::Dism),
            "Export",
            &emit,
        )
        .unwrap();
        let events = events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::OperationProgress { .. }))
                .count(),
            2
        );
        assert_eq!(
            events.iter().filter(|e| matches!(e, Event::Log(_))).count(),
            3
        );
        assert!(matches!(events.last(), Some(Event::Log(t)) if t == "Done"));
        for event in events.iter() {
            let serialized = serde_json::to_string(event).unwrap();
            let _: Event = serde_json::from_str(&serialized).unwrap();
        }
    }
    #[test]
    fn overall_counts_real_model_and_edition_steps_and_reserves_finished() {
        assert_eq!(build_steps(1, MODEL_ID), 12);
        assert_eq!(build_steps(1, "a1706-2017"), 12);
        assert_eq!(build_steps(2, "a1706-2017"), 14);
        assert_eq!(build_steps(2, MODEL_ID), 14);
        assert_eq!(build_steps(1, "a1707-2016"), 12);
        assert_eq!(build_steps(1, "a1708-2016"), 12);
        let emit: Emit = Arc::new(|event| {
            if let Event::OverallProgress { completed, total } = event {
                assert!(completed < total);
            }
        });
        let mut steps = BuildSteps::new(1, MODEL_ID, 0, &emit);
        for _ in 0..20 {
            steps.advance(&emit);
        }
    }
}

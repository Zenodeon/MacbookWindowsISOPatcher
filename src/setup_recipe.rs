//! Model-specific WinPE startup recipes. Saved model IDs remain unchanged.
use crate::domain::MODEL_ID;
const LOAD_FOLDER_2017: &str = r#"
:loadfolder2017
if not exist "%DRV%\%TAG%" (
  set "MSG=Optional startup driver unavailable: !TAG!"
  call :note
  exit /b
)
for /r "%DRV%\%TAG%" %%I in (*.inf) do (
  set "ONE=%%I"
  call :loadone
)
exit /b
"#;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupRecipe {
    A1706_2016,
    A1706_2017,
}
impl SetupRecipe {
    pub fn for_model(model: &str) -> Option<Self> {
        match model {
            MODEL_ID => Some(Self::A1706_2016),
            "a1706-2017" => Some(Self::A1706_2017),
            _ => None,
        }
    }
    pub fn model(self) -> &'static str {
        match self {
            Self::A1706_2016 => MODEL_ID,
            Self::A1706_2017 => "a1706-2017",
        }
    }
    pub fn log_folder(self) -> &'static str {
        match self {
            Self::A1706_2016 => "A1706",
            Self::A1706_2017 => "A1706-2017",
        }
    }
    pub fn graphics(self) -> &'static str {
        match self {
            Self::A1706_2016 => "Intel Iris 550",
            Self::A1706_2017 => "Intel Iris Plus 650",
        }
    }
    pub fn payload_version(self) -> &'static str {
        match self {
            Self::A1706_2016 => "a1706-v17-diagnostics-1",
            Self::A1706_2017 => "a1706-2017-external-1",
        }
    }
    pub fn startup(self) -> String {
        let original = include_str!("../payloads/a1706/a1706.cmd");
        if self == Self::A1706_2016 {
            return original.into();
        }
        let start = original
            .find("set \"PHASE=iris-readiness\"")
            .expect("readiness start");
        let end = original[start..]
            .find("if exist %SystemRoot%\\System32\\DisplaySwitch.exe (")
            .expect("display switch")
            + start;
        let readiness = r#"set "PHASE=iris-readiness"
%SystemRoot%\System32\a1706gpu2017.exe --wait-ready "!GPU!" >X:\a1706-ready.txt 2>&1
set "READY_RC=!ERRORLEVEL!"
call :addfile X:\a1706-ready.txt
if not "!READY_RC!"=="0" (
  "%LOGGER%" --event iris-readiness-failed "Iris Plus 650 did not start with the expected driver"
  echo Iris Plus 650 did not become ready. Setup has not been launched.
  echo Keep the marked logging USB connected and save the support folder.
  pause
  goto :eof
)
"#;
        let script = format!("{}{}{}", &original[..start], readiness, &original[end..]);
        let start = script
            .find("set \"TAG=cirrus4208\"")
            .expect("Apple load start");
        let end = script[start..].find(":afterdrv").expect("Apple load end") + start;
        // Bundle filenames can differ between years. Load only available INFs
        // from these allowlisted startup folders, never 2016 file names.
        let drivers = r#"for %%D in (AppleSSD64 AppleSPIDevice AppleSPIKeyboard AppleSPITrackpad AppleDFR64 CirrusAudioCS4208x64) do (
  set "TAG=%%D"
  call :loadfolder2017
)
call :cue speakers
call :cue keyboard
"#;
        let script = format!(
            "{}{}{}{}",
            &script[..start],
            drivers,
            &script[end..],
            LOAD_FOLDER_2017
        );
        script
            .replace("a1706gpu.exe", "a1706gpu2017.exe")
            .replace("\"%LOGGER%\"", "\"%LOGGER%\" --profile a1706-2017")
            .replace("Iris 550", "Iris Plus 650")
            .replace("v17 restart iris only", "2017 restart Iris Plus 650 only")
            .replace("A1706 setup log v17", "A1706 2017 external-display setup")
            .replace("MacBookPatcher-Logs.", "MacBookPatcher-Logs/A1706-2017.")
            .replace(
                r"$WinPEDriver$\AppleSPIKeyboard\AppleSPIKeyboard.inf",
                r"$WinPEDriver$\AppleSSD64\*.inf",
            )
            .replace("\r\n", "\n")
            .replace('\n', "\r\n")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(windows)]
    fn startup_loads_available_2017_bundle_filenames_with_spaces() {
        let root = std::env::temp_dir().join(format!("2017 driver loop {}", uuid::Uuid::new_v4()));
        let storage = root.join("AppleSSD64/nested/storage2017.inf");
        let touchbar = root.join("AppleDFR64/touchbar2017.inf");
        for path in [&storage, &touchbar] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "Fixture only, never loaded").unwrap();
        }
        let startup = SetupRecipe::A1706_2017.startup();
        let start = startup.find("for %%D in (AppleSSD64").unwrap();
        let end = startup[start..].find(":afterdrv").unwrap() + start;
        let script = format!(
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\nset \"DRV={}\"\r\n{}\r\nexit /b\r\n:loadone\r\necho !ONE!\r\nexit /b\r\n:note\r\nexit /b\r\n:cue\r\nexit /b\r\n{}",
            root.display(),
            &startup[start..end],
            LOAD_FOLDER_2017.replace('\n', "\r\n")
        );
        let cmd = root.join("fixture.cmd");
        std::fs::write(&cmd, script).unwrap();
        let mut command = std::process::Command::new(crate::platform::system_binary("cmd.exe"));
        let result = crate::platform::hidden(&mut command)
            .args(["/d", "/c"])
            .arg(&cmd)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = String::from_utf8_lossy(&result.stdout);
        assert!(
            output.contains(&storage.display().to_string().replace('/', "\\")),
            "{output}"
        );
        assert!(
            output.contains(&touchbar.display().to_string().replace('/', "\\")),
            "{output}"
        );
        assert_eq!(output.lines().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn profiles_do_not_change_the_2016_recipe_or_enable_other_models() {
        assert_eq!(
            SetupRecipe::A1706_2016.startup().as_bytes(),
            include_bytes!("../payloads/a1706/a1706.cmd")
        );
        assert!(SetupRecipe::for_model("a1707-2016").is_none());
        assert!(SetupRecipe::for_model("a1708-2016").is_none());
        let s = SetupRecipe::A1706_2017.startup();
        assert!(s.is_ascii() && !s.as_bytes().contains(&0));
        assert!(s.contains("--profile a1706-2017 --start"));
        assert!(s.contains("a1706gpu2017.exe --wait-ready \"!GPU!\""));
        assert!(!s.contains("a1706iris.exe"));
        assert!(!s.contains("cs4208_36.inf") && !s.contains("AppleSPIKeyboard.inf"));
        assert!(s.contains("for /r \"%DRV%\\%TAG%\" %%I in (*.inf)"));
        assert_eq!(s.matches("call :gpurestart").count(), 1);
        assert_eq!(s.matches("pnputil /restart-device").count(), 1);
        assert!(s.find("--start").unwrap() < s.find("\nwpeinit").unwrap());
        assert!(s.find("--wait-ready").unwrap() < s.find("DisplaySwitch.exe /external").unwrap());
        assert!(
            s.find("DisplaySwitch.exe /external").unwrap() < s.find("launching setup").unwrap()
        );
    }
}

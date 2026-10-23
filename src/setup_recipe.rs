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
    A1708_2016,
    A1707_2016,
}
impl SetupRecipe {
    pub fn for_model(model: &str) -> Option<Self> {
        match model {
            MODEL_ID => Some(Self::A1706_2016),
            "a1706-2017" => Some(Self::A1706_2017),
            "a1708-2016" => Some(Self::A1708_2016),
            "a1707-2016" => Some(Self::A1707_2016),
            _ => None,
        }
    }
    pub fn model(self) -> &'static str {
        match self {
            Self::A1706_2016 => MODEL_ID,
            Self::A1706_2017 => "a1706-2017",
            Self::A1708_2016 => "a1708-2016",
            Self::A1707_2016 => "a1707-2016",
        }
    }
    pub fn log_folder(self) -> &'static str {
        match self {
            Self::A1706_2016 => "A1706",
            Self::A1706_2017 => "A1706-2017",
            Self::A1708_2016 => "A1708-2016",
            Self::A1707_2016 => "A1707-2016",
        }
    }
    pub fn graphics(self) -> &'static str {
        match self {
            Self::A1706_2016 => "Intel Iris 550",
            Self::A1706_2017 => "Intel Iris Plus 650",
            Self::A1708_2016 => "Intel Iris 540",
            Self::A1707_2016 => "Radeon Pro 450/455/460",
        }
    }
    pub fn payload_version(self) -> &'static str {
        match self {
            Self::A1706_2016 => "a1706-v17-diagnostics-1",
            Self::A1706_2017 => "a1706-2017-external-1",
            Self::A1708_2016 => "a1708-2016-external-1",
            Self::A1707_2016 => "a1707-2016-external-1",
        }
    }
    pub fn startup(self) -> String {
        if matches!(self, Self::A1708_2016 | Self::A1707_2016) {
            let mut script = Self::A1706_2017
                .startup()
                .replace(
                    "a1706gpu2017.exe",
                    &format!("MacBookSetupGPU.exe --profile {}", self.model()),
                )
                .replace(
                    "--profile a1706-2017",
                    &format!("--profile {}", self.model()),
                )
                .replace("Iris Plus 650", self.graphics())
                .replace("A1706 2017", self.log_folder())
                .replace("2017 restart", "2016 restart")
                .replace("A1706 ", &format!("{} ", self.log_folder()))
                .replace("iris-readiness", "graphics-readiness")
                .replace(
                    "MacBookPatcher-Logs/A1706-2017",
                    &format!("MacBookPatcher-Logs/{}", self.log_folder()),
                );
            // Boot bundle INFs may be nested; the build validates storage contents.
            script = script.replace(
                r"$WinPEDriver$\AppleSSD64\*.inf",
                r"$WinPEDriver$\AppleSSD64\",
            );
            if self == Self::A1708_2016 {
                script = script.replace("AppleSPITrackpad AppleDFR64", "AppleSPITrackpad");
            }
            // A failed restart must not fall through to Setup. Preserve old recipes.
            script = script.replace("call :gpurestart", "call :gpurestart\r\nif not \"!GPU_RESTART_RC!\"==\"0\" goto :restartfailed")
                .replace("pnputil /restart-device \"!GPU!\" >X:\\a1706-cmd-out.txt 2>&1\r\nset RC=!ERRORLEVEL!", "pnputil /restart-device \"!GPU!\" >X:\\a1706-cmd-out.txt 2>&1\r\nset RC=!ERRORLEVEL!\r\nset \"GPU_RESTART_RC=!RC!\"");
            script.push_str("\r\n:restartfailed\r\n\"%LOGGER%\" --profile ");
            script.push_str(self.model());
            script.push_str(" --event gpu-restart-failed \"!GPU_RESTART_RC!\"\r\necho GPU restart failed. Setup has not been launched. Save the support logs.\r\npause\r\ngoto :eof\r\n");
            return script;
        }
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
    fn failed_restart_stops_setup_and_success_restarts_only_once() {
        for recipe in [SetupRecipe::A1708_2016, SetupRecipe::A1707_2016] {
            for status in [0, 5] {
                let root = std::env::temp_dir()
                    .join(format!("GPU restart fixture {}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&root).unwrap();
                let source = recipe.startup();
                let start = source.find("call :gpurestart").unwrap();
                let end = source[start..].find("set DRV=").unwrap() + start;
                let sub_start = source.find("\r\n:gpurestart\r\n").unwrap();
                let sub_end = source[sub_start..].find("\r\n:dump\r\n").unwrap() + sub_start;
                let sub = source[sub_start..sub_end]
                    .replace(
                        "pnputil /restart-device \"!GPU!\"",
                        &format!(
                            "echo restart>>\"{}\"\r\ncmd.exe /d /c exit /b {status}",
                            root.join("count.txt").display()
                        ),
                    )
                    .replace(
                        r"X:\a1706-cmd-out.txt",
                        &format!("\"{}\"", root.join("output.txt").display()),
                    );
                assert!(!sub.contains("pnputil /restart-device"));
                let script = format!(
                    "@echo off\r\nsetlocal EnableDelayedExpansion\r\n{}echo launch\r\nexit /b\r\n:restartfailed\r\necho stopped\r\nexit /b\r\n{}\r\n:note\r\n:dump\r\n:copysapi\r\n:addfile\r\nexit /b 0\r\n",
                    &source[start..end],
                    sub
                );
                let path = root.join("restart.cmd");
                std::fs::write(&path, script).unwrap();
                let mut cmd = std::process::Command::new(crate::platform::system_binary("cmd.exe"));
                let output = crate::platform::hidden(&mut cmd)
                    .args(["/d", "/c"])
                    .arg(path)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert_eq!(
                    stdout.trim(),
                    if status == 0 { "launch" } else { "stopped" }
                );
                assert_eq!(
                    std::fs::read_to_string(root.join("count.txt"))
                        .unwrap()
                        .lines()
                        .count(),
                    1
                );
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }
    #[test]
    fn new_profiles_keep_sequence_and_require_one_restart() {
        for recipe in [SetupRecipe::A1708_2016, SetupRecipe::A1707_2016] {
            let s = recipe.startup();
            assert!(s.contains(&format!(
                "MacBookSetupGPU.exe --profile {} --wait-ready",
                recipe.model()
            )));
            assert!(s.contains("set \"GPU_RESTART_RC=!RC!\""));
            assert!(s.contains("if not \"!GPU_RESTART_RC!\"==\"0\" goto :restartfailed"));
            assert_eq!(s.matches("pnputil /restart-device").count(), 1);
            assert!(s.find("--start").unwrap() < s.find("\nwpeinit").unwrap());
            assert!(
                s.find("--wait-ready").unwrap() < s.find("DisplaySwitch.exe /external").unwrap()
            );
            assert!(
                s.find("DisplaySwitch.exe /external").unwrap() < s.find("launching setup").unwrap()
            );
            assert!(
                !s.contains("a1706gpu.exe")
                    && !s.contains("a1706gpu2017.exe")
                    && !s.contains("a1706iris.exe")
            );
            assert!(!s.contains("Iris 550") && !s.contains("Iris Plus 650"));
            assert!(s.is_ascii());
            if recipe == SetupRecipe::A1708_2016 {
                assert!(!s.contains("AppleDFR64"));
            }
        }
    }
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
        assert!(SetupRecipe::for_model("a1707-2016").is_some());
        assert!(SetupRecipe::for_model("a1708-2016").is_some());
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

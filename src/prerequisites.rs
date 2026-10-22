//! Download Microsoft's ADK bootstrapper without elevation; verify before UAC.
use crate::platform;
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const ADK_SETUP_URL: &str =
    "https://download.microsoft.com/download/8e0c0f5a-abb5-4358-a51b-168eb40b1590/adk/adksetup.exe";
const MAX_SETUP_BYTES: u64 = 32 * 1024 * 1024;

fn microsoft_download(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("download.microsoft.com" | "go.microsoft.com")
        )
}

pub fn verify_microsoft(path: &Path) -> Result<()> {
    platform::powershell(
        r#"$s=Get-AuthenticodeSignature -LiteralPath $env:PATCHER_TOOL; if($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)'){throw 'A valid Microsoft signature is required'}"#,
        &[("PATCHER_TOOL", path.as_os_str())],
    ).context("Microsoft installer/tool signature verification failed")?;
    Ok(())
}

pub fn download_adk(cache: &Path, status: &dyn Fn(String)) -> Result<PathBuf> {
    let _activity = crate::cleanup::cache_activity(cache)?;
    ensure!(cfg!(windows), "ADK installation requires Windows");
    let folder = std::path::absolute(cache)?
        .join("adk-setup")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&folder)?;
    let path = folder.join("adksetup.exe");
    let part = folder.join("adksetup.part");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 || !microsoft_download(attempt.url()) {
                    attempt.error("ADK download must stay on Microsoft's HTTPS download servers")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        status("Downloading Microsoft ADK installer…".into());
        let mut response = client.get(ADK_SETUP_URL).send().await?.error_for_status()?;
        let total = response.content_length();
        ensure!(
            total.is_none_or(|n| n <= MAX_SETUP_BYTES),
            "Unexpected ADK installer size"
        );
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)?;
        let mut received = 0u64;
        let started = Instant::now();
        let mut rate = crate::transfer::Rate::default();
        let mut ticks = tokio::time::interval(Duration::from_millis(200));
        loop {
            let pending = response.chunk();
            tokio::pin!(pending);
            let next = loop {
                tokio::select! {
                    result = &mut pending => break result?,
                    _ = ticks.tick() => status(adk_progress(received, total, rate.sample(started.elapsed(), received))),
                }
            };
            let Some(chunk) = next else { break; };
            received += chunk.len() as u64;
            ensure!(
                received <= MAX_SETUP_BYTES,
                "ADK installer exceeded download limit"
            );
            file.write_all(&chunk)?;
        }
        ensure!(
            received > 0 && total.is_none_or(|n| n == received),
            "Incomplete ADK installer download"
        );
        file.sync_all()?;
        drop(file);
        status(adk_progress(received, total, rate.sample(started.elapsed(), received)));
        status("Verifying Microsoft's installer signature…".into());
        verify_microsoft(&part)?;
        fs::rename(&part, &path)?;
        Ok::<_, anyhow::Error>(path.clone())
    });
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

fn adk_progress(received: u64, total: Option<u64>, rate: Option<u64>) -> String {
    format!(
        "Downloading ADK installer: {:.1} MB{} · {}",
        received as f64 / 1_000_000.0,
        total
            .map(|n| format!(" / {:.1} MB", n as f64 / 1_000_000.0))
            .unwrap_or_default(),
        crate::transfer::label(rate)
    )
}

pub fn install_adk(cache: &Path, status: &dyn Fn(String)) -> Result<(PathBuf, bool)> {
    let _activity = crate::cleanup::cache_activity(cache)?;
    let installer = download_adk(cache, status)?;
    status("Installing ADK tools… Approve the Windows administrator prompt if shown. This may take several minutes.".into());
    // Arguments are fixed. No user-supplied command text is elevated.
    let result = platform::powershell(r#"
        $s=Get-AuthenticodeSignature -LiteralPath $env:PATCHER_SETUP
        if($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)'){throw 'The ADK installer signature is invalid'}
        $p=Start-Process -FilePath $env:PATCHER_SETUP -ArgumentList '/quiet /norestart /features OptionId.DeploymentTools' -Verb RunAs -Wait -PassThru
        $p.ExitCode
    "#,&[("PATCHER_SETUP", installer.as_os_str())]).context("ADK installation was cancelled or could not start")?;
    let exit: i32 = result
        .trim()
        .parse()
        .context("Could not read ADK installer status")?;
    ensure!(
        matches!(exit, 0 | 3010),
        "ADK installer exited with code {exit}. See the ADK logs in your temporary folder or use Microsoft’s manual installer."
    );
    let tool = platform::find_oscdimg();
    ensure!(
        tool.is_file(),
        "ADK setup finished, but oscdimg.exe was not found. Select its location manually or restart after installation."
    );
    verify_microsoft(&tool)?;
    Ok((tool, exit == 3010))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installer_progress_includes_speed_without_requiring_a_total() {
        let known = adk_progress(1_000_000, Some(2_000_000), Some(12_300_000));
        assert!(known.contains("1.0 MB / 2.0 MB · 12.3 MB/s"));
        let unknown = adk_progress(1_000_000, None, Some(0));
        assert!(unknown.contains("1.0 MB · 0.0 MB/s"));
        assert!(adk_progress(0, None, None).contains("Calculating"));
    }
    #[test]
    fn restricts_installer_redirects_to_microsoft_https() {
        for url in [
            ADK_SETUP_URL,
            "https://go.microsoft.com/fwlink/?linkid=2289980",
        ] {
            assert!(microsoft_download(&url.parse().unwrap()));
        }
        for url in [
            "http://download.microsoft.com/adk.exe",
            "https://download.microsoft.com.evil.example/adk.exe",
            "https://evil.example/adk.exe",
        ] {
            assert!(!microsoft_download(&url.parse().unwrap()));
        }
    }
}

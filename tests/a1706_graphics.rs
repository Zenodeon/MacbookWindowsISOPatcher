//! Exercise the production offline-registry logic in disposable HKCU subtrees.
//! These fixtures never open the host SYSTEM hive or its graphics services.
#![cfg(windows)]

use macbook_iso_patcher::platform;

#[test]
fn a1706_graphics_preparation_handles_fresh_and_existing_control_sets() {
    let script = format!(
        "{}\n{}",
        include_str!("../payloads/a1706/boot-graphics.ps1"),
        r#"
        function Assert($condition, $message) { if(-not $condition){throw $message} }
        function Seed($root, $current=1, $default=1) {
            $select=$root.CreateSubKey('Select')
            try {
                $select.SetValue('Current',$current,[Microsoft.Win32.RegistryValueKind]::DWord)
                $select.SetValue('Default',$default,[Microsoft.Win32.RegistryValueKind]::DWord)
                $select.SetValue('LastKnownGood',0,[Microsoft.Win32.RegistryValueKind]::DWord)
            } finally {$select.Dispose()}
            foreach($number in @($current,$default) | Sort-Object -Unique) {
                $basic=$root.CreateSubKey(('ControlSet{0:D3}\Services\BasicDisplay' -f $number))
                try {$basic.SetValue('Start',1,[Microsoft.Win32.RegistryValueKind]::DWord)} finally {$basic.Dispose()}
            }
        }
        function MustFail($action, $message) {
            $failed=$false
            try {& $action | Out-Null} catch {$failed=$true; Assert ($_.Exception.Message.Contains($message)) $_.Exception.Message}
            Assert $failed "Expected failure containing: $message"
        }
        $path='Software\Zenodeon\PatcherTests\'+[Guid]::NewGuid().ToString('N')
        $parent=[Microsoft.Win32.Registry]::CurrentUser
        $fixture=$parent.CreateSubKey($path)
        try {
            $root=$fixture.CreateSubKey('fresh')
            try {
                Seed $root
                MustFail {Set-A1706BootGraphics $root -ValidateOnly} 'igfx service is missing'
                Set-A1706BootGraphics $root
                Set-A1706BootGraphics $root -ValidateOnly
                Set-A1706BootGraphics $root # idempotent
                $service=$root.OpenSubKey('ControlSet001\Services\igfx',$true)
                try {$service.SetValue('Start',0,[Microsoft.Win32.RegistryValueKind]::DWord)} finally {$service.Dispose()}
                MustFail {Set-A1706BootGraphics $root -ValidateOnly} 'igfx Start'
                Set-A1706BootGraphics $root # correct inherited boot-start
                $service=$root.OpenSubKey('ControlSet001\Services\igfx',$true)
                try {$service.SetValue('ImagePath','C:\host\bad.sys',[Microsoft.Win32.RegistryValueKind]::ExpandString)} finally {$service.Dispose()}
                MustFail {Set-A1706BootGraphics $root -ValidateOnly} 'binary path'
                Set-A1706BootGraphics $root
            } finally {$root.Dispose()}
            $root=$fixture.CreateSubKey('alternate')
            try {
                Seed $root 2 3
                Set-A1706BootGraphics $root
                Set-A1706BootGraphics $root -ValidateOnly
                $unused=$root.OpenSubKey('ControlSet001')
                Assert ($null -eq $unused) 'Must not assume ControlSet001'
                $unrelated=$root.CreateSubKey('ControlSet002\Services\unrelated')
                try {$unrelated.SetValue('Start',2)} finally {$unrelated.Dispose()}
                Set-A1706BootGraphics $root
                $unrelated=$root.OpenSubKey('ControlSet002\Services\unrelated')
                try {Assert ($unrelated.GetValue('Start') -eq 2) 'Unrelated service changed'} finally {$unrelated.Dispose()}
            } finally {$root.Dispose()}
            $root=$fixture.CreateSubKey('2017-driverstore')
            try {
                Seed $root
                $image='\SystemRoot\System32\DriverStore\FileRepository\iigd_dch.inf_amd64_fixture\igdkmd64.sys'
                Set-A1706BootGraphics $root -ServiceImagePath $image
                Set-A1706BootGraphics $root -ServiceImagePath $image -ValidateOnly
                MustFail {Set-A1706BootGraphics $root -ValidateOnly} 'binary path'
                $service=$root.OpenSubKey('ControlSet001\Services\igfx')
                try {Assert ($service.GetValue('ImagePath','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) -eq $image) '2017 path was not preserved'} finally {$service.Dispose()}
            } finally {$root.Dispose()}
            $root=$fixture.CreateSubKey('unsafe-basic')
            try {
                Seed $root 1 2
                $basic=$root.OpenSubKey('ControlSet002\Services\BasicDisplay',$true)
                try {$basic.SetValue('Start',4)} finally {$basic.Dispose()}
                MustFail {Set-A1706BootGraphics $root} 'BasicDisplay Start'
                $service=$root.OpenSubKey('ControlSet001\Services\igfx')
                Assert ($null -eq $service) 'Validation must precede every write'
            } finally {$root.Dispose()}
            $root=$fixture.CreateSubKey('missing-control-set')
            try {
                Seed $root
                $root.DeleteSubKeyTree('ControlSet001')
                MustFail {Set-A1706BootGraphics $root} 'ControlSet001 is missing'
            } finally {$root.Dispose()}
            $root=$fixture.CreateSubKey('bad-select')
            try {
                Seed $root
                $select=$root.OpenSubKey('Select',$true)
                try {$select.SetValue('Current','1',[Microsoft.Win32.RegistryValueKind]::String)} finally {$select.Dispose()}
                MustFail {Set-A1706BootGraphics $root} 'Select\Current'
            } finally {$root.Dispose()}
            $root=$fixture.CreateSubKey('unsafe-override')
            try {
                Seed $root
                $override=$root.CreateSubKey('ControlSet001\Services\igfx\StartOverride')
                try {$override.SetValue('0',0,[Microsoft.Win32.RegistryValueKind]::DWord)} finally {$override.Dispose()}
                MustFail {Set-A1706BootGraphics $root} 'StartOverride'
            } finally {$root.Dispose()}
            'A1706 registry scenarios passed'
        } finally {
            $fixture.Dispose()
            $parent.DeleteSubKeyTree($path)
        }
        "#
    );
    let output = platform::powershell(&script, &[]).unwrap();
    assert!(output.contains("A1706 registry scenarios passed"));
}

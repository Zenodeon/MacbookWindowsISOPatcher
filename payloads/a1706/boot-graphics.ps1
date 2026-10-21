# A1706 WinPE preparation. The caller supplies only the loaded OFFLINE SYSTEM key.
# Match the working recipe: BasicDisplay remains enabled; igfx is demand-start.
function Get-A1706ControlSets([Microsoft.Win32.RegistryKey]$Root) {
    $select = $Root.OpenSubKey('Select')
    if ($null -eq $select) { throw 'Offline SYSTEM hive has no Select key' }
    try {
        $sets = @('Current', 'Default', 'LastKnownGood' | ForEach-Object {
            $value = $select.GetValue($_, $null)
            if ($null -eq $value) {
                if ($_ -ne 'LastKnownGood') { throw "Offline SYSTEM Select\$_ is missing" }
            } elseif ($select.GetValueKind($_) -ne [Microsoft.Win32.RegistryValueKind]::DWord -or $value -lt 0 -or $value -gt 999) {
                throw "Invalid offline SYSTEM Select\$_"
            } elseif ($value -eq 0) {
                if ($_ -ne 'LastKnownGood') { throw "Invalid offline SYSTEM Select\$_" }
            } else { 'ControlSet{0:D3}' -f $value }
        } | Sort-Object -Unique)
    } finally { $select.Dispose() }
    foreach ($set in $sets) {
        $key = $Root.OpenSubKey($set)
        if ($null -eq $key) { throw "Offline SYSTEM $set is missing" }
        $key.Dispose()
        $basic = $Root.OpenSubKey("$set\Services\BasicDisplay")
        if ($null -eq $basic) { throw "Offline $set BasicDisplay service is missing" }
        try {
            if ($basic.GetValue('Start', -1) -ne 1 -or $basic.GetValueKind('Start') -ne [Microsoft.Win32.RegistryValueKind]::DWord) {
                throw "Unsafe offline $set BasicDisplay Start; expected 1"
            }
        } finally { $basic.Dispose() }
        $override = $Root.OpenSubKey("$set\Services\igfx\StartOverride")
        if ($null -ne $override) {
            try {
                foreach ($value in $override.GetValueNames()) {
                    if ($override.GetValue($value) -ne 3 -or $override.GetValueKind($value) -ne [Microsoft.Win32.RegistryValueKind]::DWord) {
                        throw "Unsafe offline $set igfx StartOverride"
                    }
                }
            } finally { $override.Dispose() }
        }
    }
    return $sets
}

function Set-A1706BootGraphics([Microsoft.Win32.RegistryKey]$Root, [switch]$ValidateOnly) {
    # Validate every referenced control set before changing any of them.
    $sets = @(Get-A1706ControlSets $Root)
    foreach ($set in $sets) {
        $path = "$set\Services\igfx"
        if (-not $ValidateOnly) {
            $service = $Root.CreateSubKey($path)
            try {
                $service.SetValue('Type', 1, [Microsoft.Win32.RegistryValueKind]::DWord)
                $service.SetValue('Start', 3, [Microsoft.Win32.RegistryValueKind]::DWord)
                $service.SetValue('ErrorControl', 0, [Microsoft.Win32.RegistryValueKind]::DWord)
                $service.SetValue('Group', 'Video', [Microsoft.Win32.RegistryValueKind]::String)
                $service.SetValue('ImagePath', '\SystemRoot\System32\drivers\igdkmd64.sys', [Microsoft.Win32.RegistryValueKind]::ExpandString)
            } finally { $service.Dispose() }
        }
        $service = $Root.OpenSubKey($path)
        if ($null -eq $service) { throw "Offline $set igfx service is missing" }
        try {
            foreach ($entry in @(@('Type', 1), @('Start', 3), @('ErrorControl', 0))) {
                if ($service.GetValue($entry[0], -1) -ne $entry[1] -or $service.GetValueKind($entry[0]) -ne [Microsoft.Win32.RegistryValueKind]::DWord) {
                    throw "Incorrect offline $set igfx $($entry[0])"
                }
            }
            $image = $service.GetValue('ImagePath', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            if ($image -ne '\SystemRoot\System32\drivers\igdkmd64.sys' -or $service.GetValueKind('ImagePath') -ne [Microsoft.Win32.RegistryValueKind]::ExpandString -or $service.GetValue('Group', '') -ne 'Video') {
                throw "Incorrect offline $set igfx binary path or load group"
            }
        } finally { $service.Dispose() }
        Write-Output "A1706 $set verified: BasicDisplay Start=1; igfx Start=3 (demand), signed igdkmd64.sys"
    }
    $Root.Flush()
}

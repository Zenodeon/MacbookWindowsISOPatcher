# Called only after validating the pinned INF, catalogs and serviced kernels.
function Set-RadeonBootGraphics([Microsoft.Win32.RegistryKey]$Root, [string]$ProxyPath, [string]$KernelPath, [switch]$ValidateOnly) {
    $sets = @(Get-A1706ControlSets $Root -Services @('amdkmdap','amdkmdag'))
    foreach ($set in $sets) {
        foreach ($name in @('amdkmdap','amdkmdag')) {
            $imagePath = if ($name -eq 'amdkmdap') {$ProxyPath} else {$KernelPath}
            $path = "$set\Services\$name"
            if (-not $ValidateOnly) {
                $service = $Root.CreateSubKey($path)
                try {
                    $service.SetValue('Type',1,[Microsoft.Win32.RegistryValueKind]::DWord)
                    $service.SetValue('Start',3,[Microsoft.Win32.RegistryValueKind]::DWord)
                    $service.SetValue('ErrorControl',0,[Microsoft.Win32.RegistryValueKind]::DWord)
                    $service.SetValue('Group','Video',[Microsoft.Win32.RegistryValueKind]::String)
                    $service.SetValue('ImagePath',$imagePath,[Microsoft.Win32.RegistryValueKind]::ExpandString)
                    # Exact AddReg from the pinned ati2mtag_ServiceSettings section.
                    if ($name -eq 'amdkmdag') {$service.SetValue('PowerXpressLoad',-1,[Microsoft.Win32.RegistryValueKind]::DWord)}
                } finally {$service.Dispose()}
            }
            $service=$Root.OpenSubKey($path)
            if ($null -eq $service) {throw "Offline Radeon service missing: $set/$name"}
            try {
                foreach ($entry in @(@('Type',1),@('Start',3),@('ErrorControl',0))) {
                    if ($service.GetValue($entry[0],-999) -ne $entry[1] -or $service.GetValueKind($entry[0]) -ne [Microsoft.Win32.RegistryValueKind]::DWord) {throw "Incorrect offline Radeon $name/$($entry[0])"}
                }
                if ($service.GetValue('ImagePath','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) -ne $imagePath -or $service.GetValueKind('ImagePath') -ne [Microsoft.Win32.RegistryValueKind]::ExpandString -or $service.GetValue('Group','') -ne 'Video') {throw "Incorrect offline Radeon $name image path/group"}
                if ($name -eq 'amdkmdag' -and ($service.GetValue('PowerXpressLoad',0) -ne -1 -or $service.GetValueKind('PowerXpressLoad') -ne [Microsoft.Win32.RegistryValueKind]::DWord)) {throw 'Incorrect Radeon PowerXpressLoad'}
            } finally {$service.Dispose()}
            Write-Output "$set verified: BasicDisplay enabled; $name demand-start from $imagePath"
        }
    }
    $Root.Flush()
}

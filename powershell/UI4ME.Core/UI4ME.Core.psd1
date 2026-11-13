@{
    RootModule = 'UI4ME.Core.psm1'
    ModuleVersion = '1.0.0'
    GUID = '16eaf035-4e6b-4b37-abac-f684c8f00141'
    Author = 'UI4ME contributors'
    CompanyName = 'UI4ME'
    Copyright = '(c) 2026 UI4ME contributors. MIT license.'
    Description = 'Native HTML/CSS desktop windows for PowerShell 7 on Linux/Windows and Windows PowerShell 5.1. Rust host, no browser, JavaScript, Python runtime or HTTP server.'
    PowerShellVersion = '5.1'
    DotNetFrameworkVersion = '4.7.2'
    CompatiblePSEditions = @('Desktop','Core')
    NestedModules = @()
    RequiredAssemblies = @()
    FunctionsToExport = @(
        'Connect-UI4ME', 'Close-UI4ME', 'Wait-UI4ME', 'Invoke-UI4MERpc',
        'Send-UI4MENotification', 'Start-UI4MERpc', 'Wait-UI4MERpc', 'Stop-UI4MERpc',
        'New-UI4MEWindow', 'Set-UI4MEWindow', 'Get-UI4MEWindow', 'Get-UI4MEWindowList',
        'Close-UI4MEWindow', 'Request-UI4MEAttention', 'Set-UI4MEBeforeClose', 'Set-UI4MEBeforeQuit',
        'Set-UI4MEReady', 'Request-UI4MEQuit', 'Register-UI4MENotification', 'Open-UI4MEHtml',
        'Open-UI4MEFile', 'Remove-UI4MEDocument', 'Register-UI4MEDocumentEvent', 'Unregister-UI4MEDocumentEvent',
        'Invoke-UI4MEDocumentCommand', 'Invoke-UI4MEBatch', 'Get-UI4MEDocumentText', 'Set-UI4MEDocumentText',
        'Get-UI4MEValue', 'Set-UI4MEDocumentValue', 'Get-UI4MEChecked', 'Set-UI4MEChecked',
        'Set-UI4MEDocumentAttribute', 'Set-UI4MEClass', 'Set-UI4MEDocumentHtml', 'Remove-UI4MEElement',
        'Find-UI4MEElement', 'Add-UI4MEStylesheet', 'Get-UI4MEStylesheets', 'Set-UI4MEStylesheet',
        'Get-UI4MEComputedStyle', 'Get-UI4MEEnvironment', 'Set-UI4MEEnvironment', 'Get-UI4MEScroll',
        'Set-UI4MEScroll', 'Get-UI4MEFocusedElement', 'Set-UI4MEFocus', 'Get-UI4MESelection',
        'Set-UI4MESelection', 'Invoke-UI4MEEditCommand', 'Get-UI4MEFormData', 'Submit-UI4MEForm',
        'Reset-UI4MEForm', 'Get-UI4MESelectOptions', 'Set-UI4MESelectedValues', 'Show-UI4MEPopover',
        'Show-UI4MEHtmlDialog', 'Set-UI4MEPreventDefault', 'Get-UI4MEDiagnostics', 'Get-UI4MEResourceStats',
        'Clear-UI4MEUnusedResources', 'Register-UI4MEResources', 'Unregister-UI4MEResources', 'Get-UI4MENode',
        'Get-UI4MENodes', 'Get-UI4MENodeInfo', 'Get-UI4MENodeChildren', 'Invoke-UI4MENodeBatch',
        'Invoke-UI4MENodeOperation', 'Get-UI4MECapabilities', 'Get-UI4MEPreferences', 'Get-UI4MEClipboard',
        'Set-UI4MEClipboard', 'Open-UI4MEExternal', 'Show-UI4MEFileDialog', 'New-UI4MELiveBitmap',
        'Send-UI4MELiveBitmap', 'Remove-UI4MELiveBitmap', 'Get-UI4MEHostPath', 'Start-UI4MEHost',
        'New-UI4MEProject', 'Install-UI4MESkill'
    )
    CmdletsToExport = @()
    AliasesToExport = @()
    PrivateData = @{
        PSData = @{
            Tags = @('UI','GUI','HTML','CSS','Native','Desktop','Linux','Windows','Wayland','PowerShell','Rust','PSEdition_Desktop','PSEdition_Core')
            LicenseUri = 'https://github.com/ui4me/ui4me-core/blob/main/LICENSE'
            ProjectUri = 'https://github.com/ui4me/ui4me-core'
            ReleaseNotes = 'First release of the PowerShell binding for UI4ME Core 1.0.0. Bundles the same qualified native hosts as the ui4me-core 1.0.0 Python wheels (Linux x64, Windows x64).'
        }
    }
}

# UI4ME Showcase: a multi-page tour of the HTML and CSS that UI4ME Core supports.
#
# Run it directly:
#   pwsh -File ./app.ps1                          (PowerShell 7.2+, Linux or Windows)
#   powershell.exe -NoProfile -File .\app.ps1     (Windows PowerShell 5.1)
#
# The interface is ui/index.html and ui/style.css. This script is the backend:
# it switches pages, toggles the theme, reads the form, opens the dialog, shows
# toasts and reacts to links. Copy the folder as a starting point.
#
# Start-UI4MEScript starts the native window host, which runs this script a
# second time as its backend. Everything above that call runs twice, so keep
# real work below it.
$ErrorActionPreference = 'Stop'
# Use the installed module; from a source checkout, fall back to the module here.
try { Import-Module UI4ME.Core -MinimumVersion 1.0.1 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }

$app = Start-UI4MEScript -Name 'UI4ME Showcase' -AppId 'org.ui4me.examples.ShowcasePS'

$UiFolder = Join-Path $PSScriptRoot 'ui'

# Page name -> title, subtitle. Each page is a <section id="page-NAME">,
# and its sidebar button is <button id="nav-NAME">.
$Pages = [ordered]@{
    overview = @('Overview', 'Cards, stat tiles, badges, progress bars and a table.')
    forms    = @('Forms', 'Inputs, selects, checkboxes, radios, validation and submit.')
    layouts  = @('Layouts', 'Flexbox, grid areas, a holy-grail layout and sticky headings.')
    content  = @('Typography & content', 'Headings, lists, quotes, code, links and an accordion.')
    overlays = @('Feedback & overlays', 'Dialog, popover, toasts, a counter and the theme.')
}
# Labels for the validation message, by input id.
$FieldLabels = @{
    'full-name' = 'Full name'; 'email' = 'Email'; 'username' = 'Username'
    'password' = 'Password'; 'website' = 'Website'; 'team-size' = 'Team size'; 'terms' = 'Terms'
}
$TakenUsernames = @('admin', 'root', 'ui4me')
$Toasts = @{
    success = @('Saved', 'Everything went well.')
    warning = @('Check this', 'Something needs your attention.')
    error   = @('Failed', 'The action could not be completed.')
}

# Application state. Callbacks change it through $script: variables.
$script:doc = $null
$script:page = 'overview'
$script:theme = 'system'      # 'system', 'light' or 'dark'
$script:counter = 0
$script:progress = 64
$script:toastCount = 0
$script:invalidFields = New-Object System.Collections.ArrayList

# ----- Helpers ---------------------------------------------------------------

function Set-Status([string]$Text) {
    Set-UI4MEDocumentText -Document $script:doc -Element status -Text $Text
}

function Show-Page([string]$Name) {
    # Show one <section class="page"> and hide the others, in one atomic batch.
    if (-not $Pages.Contains($Name)) { return }
    $script:page = $Name
    $ops = @()
    foreach ($p in $Pages.Keys) {
        $change = if ($p -eq $Name) { 'add_class' } else { 'remove_class' }
        $current = if ($p -eq $Name) { 'page' } else { 'false' }
        $ops += @{ op = $change; element = "page-$p"; name = 'active' }
        $ops += @{ op = $change; element = "nav-$p"; name = 'active' }
        $ops += @{ op = 'set_attribute'; element = "nav-$p"; name = 'aria-current'; value = $current }
    }
    $ops += @{ op = 'set_text'; element = 'page-title'; text = $Pages[$Name][0] }
    $ops += @{ op = 'set_text'; element = 'page-subtitle'; text = $Pages[$Name][1] }
    $ops += @{ op = 'set_text'; element = 'status-page'; text = $Pages[$Name][0] }
    Invoke-UI4MEBatch -Document $script:doc -Operations $ops
    $null = Set-UI4MEScroll -Document $script:doc -Element pages -Y 0
}

function Get-EffectiveScheme {
    # 'light' or 'dark': the forced theme, or what the desktop prefers.
    if ($script:theme -ne 'system') { return $script:theme }
    return [string](Get-UI4MEEnvironment -Document $script:doc).color_scheme
}

function Test-Element([string]$Id) {
    return ((Find-UI4MEElement -Document $script:doc -Selector "#$Id").total -gt 0)
}

function Update-Theme {
    $d = $script:doc
    # style.css: .theme-light / .theme-dark force a color-scheme; theme-system follows the desktop.
    Set-UI4MEDocumentAttribute -Document $d -Element app -Name class -Value "theme-$($script:theme)"
    if (Test-Element 'theme-toggle') {
        $scheme = Get-EffectiveScheme
        Set-UI4MEChecked -Document $d -Element "theme-mode-$($script:theme)" -Checked $true
        $label = if ($scheme -eq 'dark') { 'Light mode' } else { 'Dark mode' }
        Set-UI4MEDocumentText -Document $d -Element theme-label -Text $label
        Set-UI4MEDocumentText -Document $d -Element stat-theme -Text ($scheme.Substring(0, 1).ToUpper() + $scheme.Substring(1))
        $source = if ($script:theme -eq 'system') { 'Follows the desktop' } else { 'Chosen in the app' }
        Set-UI4MEDocumentText -Document $d -Element stat-theme-source -Text $source
    }
}

function Show-Counter {
    $d = $script:doc
    Set-UI4MEDocumentText -Document $d -Element counter-value -Text "$($script:counter)"
    Set-UI4MEDocumentText -Document $d -Element stat-counter -Text "$($script:counter)"
    Set-UI4MEClass -Document $d -Element counter-value -Name negative -Remove:($script:counter -ge 0)
}

function Show-Progress {
    $d = $script:doc
    Set-UI4MEDocumentAttribute -Document $d -Element build-progress -Name value -Value "$($script:progress)"
    Set-UI4MEDocumentText -Document $d -Element progress-label -Text "$($script:progress)%"
    Set-UI4MEDocumentText -Document $d -Element stat-progress -Text "$($script:progress)%"
}

# ----- Navigation, theme and links -------------------------------------------

$onNavigate = {
    param($e)
    # One handler for the whole <nav>: $e.element is the button's id.
    if ($e.element.StartsWith('nav-')) {
        Show-Page $e.element.Substring(4)
        Set-Status "Showing the $($Pages[$script:page][0]) page."
    }
}

$onGoForms = {
    param($e)
    Show-Page 'forms'
    Set-UI4MEFocus -Document $script:doc -Element full-name
}

$onThemeToggle = {
    param($e)
    $script:theme = if ((Get-EffectiveScheme) -eq 'light') { 'dark' } else { 'light' }
    Update-Theme
    Set-Status "Theme set to $($script:theme)."
}

$onThemeChoice = {
    param($e)
    if ($e.element.StartsWith('theme-mode-') -and $e.checked) {
        $script:theme = $e.element.Substring(11)
        Update-Theme
        Set-Status "Theme: $($script:theme)."
    }
}

$onEnvironmentChanged = {
    param($e)
    # The desktop switched between light and dark.
    Update-Theme
}

$onLink = {
    param($e)
    $href = [string]$e.href
    if ($href.StartsWith('#')) {
        Show-Page $href.Substring(1)
        Set-Status "Link to $href handled by the backend."
    } elseif ($href -eq 'about.html') {
        Open-About
    } elseif ($href -eq 'index.html') {
        Open-Main
        Set-Status 'Back from about.html: the page, theme and counter were restored.'
    } else {
        # A real application could call Open-UI4MEExternal -Application $app -Target $e.url
        Set-Status "Link activated: $($e.url) (not opened in this demo)."
    }
}

$onAdvance = {
    param($e)
    $script:progress = if ($script:progress -ge 100) { 0 } else { [Math]::Min(100, $script:progress + 12) }
    Show-Progress
    Set-Status "Build progress: $($script:progress)%."
}

# ----- Forms -----------------------------------------------------------------

$onFormInput = {
    param($e)
    $d = $script:doc
    $script:invalidFields.Clear()
    $value = [string]$e.value
    if ($e.element -eq 'volume') {
        Set-UI4MEDocumentText -Document $d -Element volume-out -Text $value
    } elseif ($e.element -eq 'username') {
        if ($TakenUsernames -contains $value.ToLowerInvariant()) {
            $null = Invoke-UI4MEDocumentCommand -Document $d -Operation set_custom_validity -Parameters @{ element = 'username'; message = 'That username is already taken.' }
            Set-UI4MEDocumentText -Document $d -Element username-hint -Text 'That username is already taken.'
            Set-UI4MEClass -Document $d -Element username-hint -Name error
        } else {
            $null = Invoke-UI4MEDocumentCommand -Document $d -Operation set_custom_validity -Parameters @{ element = 'username'; message = '' }
            Set-UI4MEDocumentText -Document $d -Element username-hint -Text 'Try "admin": the backend marks it as taken.'
            Set-UI4MEClass -Document $d -Element username-hint -Name error -Remove
        }
    }
}

$onInvalid = {
    param($e)
    # One event per invalid control, sent when a submit attempt fails validation.
    $label = $FieldLabels[[string]$e.element]
    if (-not $label) { $label = [string]$e.element }
    if (-not $script:invalidFields.Contains($label)) { [void]$script:invalidFields.Add($label) }
    $d = $script:doc
    Set-UI4MEClass -Document $d -Element signup -Name show-errors
    Set-UI4MEDocumentAttribute -Document $d -Element form-feedback -Name class -Value 'feedback error'
    Set-UI4MEDocumentText -Document $d -Element form-feedback -Text ('Please check: ' + ($script:invalidFields -join ', ') + '.')
}

$onSubmit = {
    param($e)
    $d = $script:doc
    $script:invalidFields.Clear()
    # Entries arrive in document order; repeated names (checkboxes, multi-select) repeat.
    $values = [ordered]@{}
    foreach ($entry in (Get-UI4MEFormData -Document $d -Form signup).entries) {
        $value = [string]$entry.value
        if ($entry.name -eq 'password') { $value = '*' * $value.Length }
        if (-not $values.Contains($entry.name)) { $values[$entry.name] = New-Object System.Collections.ArrayList }
        [void]$values[$entry.name].Add($value)
    }
    $rows = foreach ($name in $values.Keys) {
        $joined = $values[$name] -join ', '
        if (-not $joined) { $joined = '-' }
        '<dt>' + [Net.WebUtility]::HtmlEncode($name) + '</dt><dd>' + [Net.WebUtility]::HtmlEncode($joined) + '</dd>'
    }
    Set-UI4MEDocumentHtml -Document $d -Element form-result -Html (@($rows) -join '')
    Set-UI4MEDocumentText -Document $d -Element result-badge -Text "$($values.Count) fields"
    Set-UI4MEDocumentAttribute -Document $d -Element result-badge -Name class -Value 'badge success'
    Set-UI4MEClass -Document $d -Element signup -Name show-errors -Remove
    Set-UI4MEDocumentAttribute -Document $d -Element form-feedback -Name class -Value 'feedback success'
    Set-UI4MEDocumentText -Document $d -Element form-feedback -Text 'Submitted. The backend read the form with Get-UI4MEFormData.'
    Set-Status 'Form submitted.'
}

$onFormReset = {
    param($e)
    $d = $script:doc
    $null = Invoke-UI4MEDocumentCommand -Document $d -Operation set_custom_validity -Parameters @{ element = 'username'; message = '' }
    Set-UI4MEDocumentText -Document $d -Element volume-out -Text (Get-UI4MEValue -Document $d -Element volume)
    Set-UI4MEClass -Document $d -Element signup -Name show-errors -Remove
    Set-UI4MEDocumentAttribute -Document $d -Element form-feedback -Name class -Value 'feedback'
    Set-UI4MEDocumentText -Document $d -Element form-feedback -Text 'The form was reset to its default values.'
    Set-Status 'Form reset.'
}

# ----- Overlays --------------------------------------------------------------

$onOpenDialog = {
    param($e)
    Show-UI4MEHtmlDialog -Document $script:doc -Element demo-dialog -Modal $true
}

$onDialogButton = {
    param($e)
    if ($e.element -eq 'dialog-confirm') {
        Show-UI4MEHtmlDialog -Document $script:doc -Element demo-dialog -Close -ReturnValue 'published'
    } elseif ($e.element -eq 'dialog-cancel') {
        Show-UI4MEHtmlDialog -Document $script:doc -Element demo-dialog -Close -ReturnValue 'cancelled'
    }
}

$onDialogClosed = {
    param($e)
    $answer = [string]$e.return_value
    if (-not $answer) { $answer = 'dismissed' }
    if ($answer -eq 'published') {
        $note = Get-UI4MEValue -Document $script:doc -Element dialog-note
        $text = "Published with the note `"$note`"."
    } else {
        $text = "Dialog $answer."
    }
    Set-UI4MEDocumentText -Document $script:doc -Element dialog-result -Text $text
    Set-Status $text
}

$onTogglePopover = {
    param($e)
    # Returns the new state; the "toggle" event below reports it as well.
    $null = Show-UI4MEPopover -Document $script:doc -Element tips-popover -Action toggle
}

$onPopoverToggled = {
    param($e)
    $state = [string]$e.new_state
    if (-not $state) { $state = 'toggled' }
    Set-Status "The popover is now $state."
}

$onCounter = {
    param($e)
    $changes = @{ 'counter-inc' = 1; 'counter-dec' = -1 }
    if ($e.element -eq 'counter-reset') {
        $script:counter = 0
    } elseif ($changes.ContainsKey([string]$e.element)) {
        $script:counter += $changes[[string]$e.element]
    } else {
        return
    }
    Show-Counter
}

$onToastButton = {
    param($e)
    $kind = ([string]$e.element) -replace '^toast-', ''
    if ($kind -eq 'clear') {
        Set-UI4MEDocumentHtml -Document $script:doc -Element toasts -Html ''
        return
    }
    if (-not $Toasts.ContainsKey($kind)) { return }
    $script:toastCount++
    $number = $script:toastCount
    $title = $Toasts[$kind][0]
    $text = $Toasts[$kind][1]
    $toast = "<div id=`"toast-$number`" class=`"toast $kind`" role=`"status`">" +
        "<div class=`"toast-body`"><strong>$title</strong><span>$text (#$number)</span></div>" +
        "<button id=`"toast-close-$number`" class=`"toast-close`" type=`"button`" aria-label=`"Dismiss`">" +
        '&times;</button></div>'
    # Add the new toast and keep only the newest three, in one transaction.
    Invoke-UI4MEBatch -Document $script:doc -Operations @(
        @{ op = 'insert_html'; element = 'toasts'; position = 'beforeend'; html = $toast },
        @{ op = 'trim_children'; element = 'toasts'; keep = 3; from = 'start' }
    )
}

$onToastClose = {
    param($e)
    $id = [string]$e.element
    if ($id.StartsWith('toast-close-')) {
        Remove-UI4MEElement -Document $script:doc -Element ('toast-' + $id.Substring(12))
    }
}

# ----- Loading documents -----------------------------------------------------

function Open-Main {
    # Load index.html and connect every handler (also used by 'Back').
    $script:doc = Open-UI4MEFile -Window $window -Path (Join-Path $UiFolder 'index.html')
    $d = $script:doc
    # A form submit would otherwise be a native action; prevent it up front.
    Set-UI4MEPreventDefault -Document $d -Element signup -EventType submit

    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element nav -Callback $onNavigate
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element go-forms -Callback $onGoForms
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element theme-toggle -Callback $onThemeToggle
    $null = Register-UI4MEDocumentEvent -Document $d -Type change -Element theme-choice -Callback $onThemeChoice
    $null = Register-UI4MEDocumentEvent -Document $d -Type environment_changed -Callback $onEnvironmentChanged
    $null = Register-UI4MEDocumentEvent -Document $d -Type link_activated -Callback $onLink
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element advance -Callback $onAdvance
    # Forms page
    $null = Register-UI4MEDocumentEvent -Document $d -Type submit -Element signup -Callback $onSubmit
    $null = Register-UI4MEDocumentEvent -Document $d -Type invalid -Callback $onInvalid   # invalid does not bubble
    $null = Register-UI4MEDocumentEvent -Document $d -Type input -Element signup -Callback $onFormInput
    $null = Register-UI4MEDocumentEvent -Document $d -Type reset -Element signup -Callback $onFormReset
    # Overlays page
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element open-dialog -Callback $onOpenDialog
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element demo-dialog -Callback $onDialogButton
    $null = Register-UI4MEDocumentEvent -Document $d -Type close -Element demo-dialog -Callback $onDialogClosed
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element toggle-popover -Callback $onTogglePopover
    $null = Register-UI4MEDocumentEvent -Document $d -Type toggle -Element tips-popover -Callback $onPopoverToggled
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element counter-controls -Callback $onCounter
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element counter-reset -Callback $onCounter
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element toast-buttons -Callback $onToastButton
    $null = Register-UI4MEDocumentEvent -Document $d -Type click -Element toasts -Callback $onToastClose

    # Restore what the backend remembers (matters after returning from about.html).
    $os = if ($IsLinux) { 'Linux' } elseif ($IsMacOS) { 'macOS' } else { 'Windows' }
    Set-UI4MEDocumentText -Document $d -Element backend-name -Text "PowerShell $($PSVersionTable.PSVersion) on $os"
    Update-Theme
    Show-Page $script:page
    Show-Counter
    Show-Progress
}

function Open-About {
    # Load a second HTML file into the same window.
    $script:doc = Open-UI4MEFile -Window $window -Path (Join-Path $UiFolder 'about.html')
    $null = Register-UI4MEDocumentEvent -Document $script:doc -Type link_activated -Callback $onLink
    Update-Theme
}

# ----- Start -----------------------------------------------------------------

$window = New-UI4MEWindow -Application $app -Title 'UI4ME Showcase' -Width 1100 -Height 760 -MinWidth 640 -MinHeight 480
Open-Main
Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app

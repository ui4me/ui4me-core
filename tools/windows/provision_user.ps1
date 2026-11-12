[CmdletBinding()]
param()
# Read one JSON object from stdin. Do not echo it: it contains a password.
# Fields: username, password, workspace, public_key; optional reuse_existing=true.
# Run in the administrator SSH session, not as the test user.
$ErrorActionPreference = 'Stop'
$inputRecord = [Console]::In.ReadToEnd() | ConvertFrom-Json
$userName = [string]$inputRecord.username
$workspace = [string]$inputRecord.workspace
if ($userName -notmatch '^[A-Za-z][A-Za-z0-9_-]{0,19}$') { throw 'Invalid dedicated development username' }
if ($workspace -notmatch '^[Ff]:\\[^\r\n]+$') { throw 'The dedicated workspace must be an absolute path on F:' }
if (-not (Test-Path -LiteralPath 'F:\')) { throw 'F: does not exist; do not substitute another disk silently.' }
$user = Get-LocalUser -Name $userName -ErrorAction SilentlyContinue
if ($user -and -not $inputRecord.reuse_existing) { throw 'Account already exists; explicitly reuse its known credential record.' }
if (-not $user) {
    if ([string]$inputRecord.password -eq '') { throw 'A generated password is required for the new account' }
    $secure = ConvertTo-SecureString ([string]$inputRecord.password) -AsPlainText -Force
    $user = New-LocalUser -Name $userName -Password $secure -Description 'UI4ME development and native desktop verification'
}
foreach ($sid in @('S-1-5-32-545', 'S-1-5-32-555')) {
    $group = Get-LocalGroup -SID $sid
    $member = Get-LocalGroupMember -Group $group -ErrorAction SilentlyContinue | Where-Object { $_.SID -eq $user.SID }
    if (-not $member) { Add-LocalGroupMember -Group $group -Member $user }
}
New-Item -ItemType Directory -Path $workspace -Force | Out-Null
& icacls.exe $workspace /grant ('*' + $user.SID.Value + ':(OI)(CI)M') | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot grant access to the dedicated workspace' }
if ($inputRecord.public_key) {
    $profile = Get-CimInstance Win32_UserProfile | Where-Object { $_.SID -eq $user.SID.Value } | Select-Object -First 1
    if ($profile) {
        $profileDir = $profile.LocalPath
    } else {
        Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class Ui4meUserProfile {
    [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
    public static extern int CreateProfile(string sid, string name, StringBuilder path, uint size);
}
'@
        $profilePath = [System.Text.StringBuilder]::new(1024)
        $profileResult = [Ui4meUserProfile]::CreateProfile($user.SID.Value, $userName, $profilePath, 1024)
        if ($profileResult -ne 0) { throw ('Could not create the user profile: HRESULT 0x{0:X8}' -f $profileResult) }
        $profileDir = $profilePath.ToString()
    }
    $sshDir = Join-Path $profileDir '.ssh'
    New-Item -ItemType Directory -Force -Path $sshDir | Out-Null
    $authorized = Join-Path $sshDir 'authorized_keys'
    $publicKey = ([string]$inputRecord.public_key).Trim()
    if ($publicKey -notmatch '^ssh-(ed25519|rsa) [A-Za-z0-9+/=]+(?: .*)?$' -and $publicKey -notmatch '^ecdsa-sha2-[^ ]+ [A-Za-z0-9+/=]+(?: .*)?$') {
        throw 'Expected a single OpenSSH public key'
    }
    $lines = if (Test-Path -LiteralPath $authorized) { @(Get-Content -LiteralPath $authorized) } else { @() }
    if ($publicKey -notin $lines) { Add-Content -LiteralPath $authorized -Value $publicKey -Encoding ASCII }
    & icacls.exe $sshDir /inheritance:r /grant ('*' + $user.SID.Value + ':(OI)(CI)F') '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot set SSH directory permissions' }
    & icacls.exe $authorized /inheritance:r /grant ('*' + $user.SID.Value + ':F') '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot set authorized-key permissions' }
}
@{ username=$userName; workspace=$workspace; sid=$user.SID.Value; status='provisioned'; rdp_required=$true } | ConvertTo-Json -Compress

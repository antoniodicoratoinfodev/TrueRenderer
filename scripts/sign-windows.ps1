# Sign a prebuilt Windows application directory, or verify it without modification.
# The certificate must come from a provider trusted by Smart App Control.
[CmdletBinding(DefaultParameterSetName = 'Verify')]
param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryDirectory,
    [Parameter(Mandatory = $true, ParameterSetName = 'Sign')]
    [ValidatePattern('^[A-Fa-f0-9]{40}$')]
    [string]$CertificateThumbprint,
    [Parameter(Mandatory = $true, ParameterSetName = 'Sign')]
    [uri]$TimestampUrl,
    [Parameter(Mandatory = $true, ParameterSetName = 'Sign')]
    [string]$SignTool,
    [Parameter(Mandatory = $true, ParameterSetName = 'Verify')]
    [switch]$VerifyOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-SignedBinary([string]$File) {
    $signature = Get-AuthenticodeSignature -LiteralPath $File
    if ($signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate) {
        throw "Invalid or missing Authenticode signature: $File ($($signature.Status))"
    }
    # Catalog signatures may not travel with a copied worker or application.
    if ($signature.SignatureType -ne 'Authenticode') {
        throw "An embedded Authenticode signature is required: $File"
    }
    if ($null -eq $signature.TimeStamperCertificate) {
        throw "A trusted timestamp is required: $File"
    }
    if ($signature.SignerCertificate.Subject -eq $signature.SignerCertificate.Issuer) {
        throw "Self-signed publisher certificates are not supported: $File"
    }
    [pscustomobject]@{
        File = $File
        SHA256 = (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash
        Publisher = $signature.SignerCertificate.Subject
        Thumbprint = $signature.SignerCertificate.Thumbprint
    }
}

$directory = Get-Item -LiteralPath $BinaryDirectory
if (-not $directory.PSIsContainer) { throw 'BinaryDirectory must be a directory.' }
$entries = @($directory) + @(Get-ChildItem -LiteralPath $directory.FullName -Recurse -Force)
if (@($entries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) {
    throw 'Use a staging directory without links or junctions.'
}
foreach ($required in @('truerenderer.exe', 'tr-worker.exe')) {
    if (-not (Test-Path -LiteralPath (Join-Path $directory.FullName $required) -PathType Leaf)) {
        throw "Missing application binary: $required"
    }
}
$binaries = @($entries | Where-Object {
    -not $_.PSIsContainer -and $_.Extension -in @('.exe', '.dll')
} | Sort-Object FullName)

if ($PSCmdlet.ParameterSetName -eq 'Sign') {
    if (-not $TimestampUrl.IsAbsoluteUri -or $TimestampUrl.Scheme -ne 'https') {
        throw 'Use the HTTPS RFC 3161 timestamp endpoint supplied by your signing provider.'
    }
    $signToolPath = (Get-Item -LiteralPath $SignTool).FullName
    $certificate = Get-Item -LiteralPath "Cert:\CurrentUser\My\$CertificateThumbprint"
    if (-not $certificate.HasPrivateKey -or $certificate.NotAfter -le (Get-Date) -or
        $certificate.NotBefore -gt (Get-Date) -or $certificate.Subject -eq $certificate.Issuer) {
        throw 'A current, CA-issued code-signing certificate with an accessible private key is required.'
    }
    $usages = @($certificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' } |
        ForEach-Object { $_.EnhancedKeyUsages } | ForEach-Object { $_.Value })
    if ('1.3.6.1.5.5.7.3.3' -notin $usages) {
        throw 'The certificate does not authorize code signing.'
    }
    foreach ($binary in $binaries) {
        & $signToolPath sign /sha1 $CertificateThumbprint /s My /fd SHA256 /tr $TimestampUrl.AbsoluteUri /td SHA256 $binary.FullName
        if ($LASTEXITCODE -ne 0) { throw "SignTool failed for $($binary.FullName): $LASTEXITCODE" }
        & $signToolPath verify /pa /all /tw $binary.FullName
        if ($LASTEXITCODE -ne 0) { throw "SignTool verification failed for $($binary.FullName): $LASTEXITCODE" }
        $null = Assert-SignedBinary $binary.FullName
    }
}

# Only report success once every executable and DLL has passed verification.
$results = @($binaries | ForEach-Object { Assert-SignedBinary $_.FullName })
$results
Write-Host "Verified $($results.Count) embedded, timestamped signatures against this computer's trust store."
Write-Host 'Public-CA eligibility and a native Smart App Control test are still required; this is not a release qualification.'

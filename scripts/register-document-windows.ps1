param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
$trExecutable = (Resolve-Path -LiteralPath $Executable).Path
if ([IO.Path]::GetExtension($trExecutable) -ne '.exe') { throw 'Specificare TrueRenderer.exe verificato.' }
$trClasses = 'HKCU:\Software\Classes'
$trProgId = 'TrueRenderer.Document'
foreach ($trKey in @("$trClasses\$trProgId", "$trClasses\$trProgId\DefaultIcon", "$trClasses\$trProgId\shell\open\command", "$trClasses\.truerenderer\OpenWithProgids")) {
    New-Item -Path $trKey -Force | Out-Null
}
Set-Item -Path "$trClasses\$trProgId" -Value 'Documento TrueRenderer'
Set-Item -Path "$trClasses\$trProgId\DefaultIcon" -Value ('"' + $trExecutable + '",0')
Set-Item -Path "$trClasses\$trProgId\shell\open\command" -Value ('"' + $trExecutable + '" --open "%1"')
New-ItemProperty -Path "$trClasses\.truerenderer\OpenWithProgids" -Name $trProgId -PropertyType String -Value '' -Force | Out-Null
# Preserve an existing application choice. Windows remains responsible for UserChoice.
if (-not (Get-Item -Path "$trClasses\.truerenderer").GetValue('')) {
    Set-Item -Path "$trClasses\.truerenderer" -Value $trProgId
}

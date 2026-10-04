<#
.SYNOPSIS
  Generates a passphrase from the EFF large wordlist using the operating system's
  cryptographic random number generator (unbiased, via rejection sampling).

.NOTES
  Works in Windows PowerShell 5.1 (built into Windows 10) and PowerShell 7+.

  1. Download the wordlist next to this script:
     https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt
  2. Run:
     powershell -ExecutionPolicy Bypass -File .\generate-passphrase.ps1
  3. Write the result on paper. Never paste it into a chat, email, note app or file.
#>
param(
    [string]$WordList = (Join-Path $PSScriptRoot 'eff_large_wordlist.txt'),
    [ValidateRange(5, 10)][int]$Words = 5
)

$ErrorActionPreference = 'Stop'

$list = @(Get-Content -LiteralPath $WordList -Encoding UTF8 |
    Where-Object { $_ -match '^\d{5}\s+\S+$' } |
    ForEach-Object { ($_ -split '\s+')[1] })

if ($list.Count -ne 7776) {
    throw "Expected 7776 words, found $($list.Count). Is this the EFF large wordlist?"
}

$rng   = [System.Security.Cryptography.RandomNumberGenerator]::Create()
$buf   = New-Object byte[] 4
$n     = [uint64]$list.Count
$range = [uint64]4294967296
$limit = $range - ($range % $n)   # values >= limit are discarded to avoid modulo bias

$picked = for ($i = 0; $i -lt $Words; $i++) {
    do {
        $rng.GetBytes($buf)
        $r = [uint64][BitConverter]::ToUInt32($buf, 0)
    } while ($r -ge $limit)
    $list[[int]($r % $n)]
}
$rng.Dispose()

$bits = [math]::Round($Words * [math]::Log(7776, 2), 1)
Write-Host ""
Write-Host ($picked -join ' ')
Write-Host ""
Write-Host "($Words words, about $bits bits of entropy) - write it on paper, don't save or paste it anywhere."

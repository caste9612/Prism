Add-Type -AssemblyName System.Drawing

$iconsDir = "D:\Progetti\Github\Prism\src-tauri\icons"
New-Item -ItemType Directory -Force -Path $iconsDir | Out-Null

function Create-SimpleIcon {
    param([int]$size, [string]$path)

    $bmp = New-Object System.Drawing.Bitmap($size, $size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'

    # Blue background
    $blueBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(14, 165, 233))
    $g.FillRectangle($blueBrush, 0, 0, $size, $size)

    # White "P" letter
    $fontSize = [int]($size * 0.55)
    $font = New-Object System.Drawing.Font("Segoe UI", $fontSize, [System.Drawing.FontStyle]::Bold)
    $whiteBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)

    $sf = New-Object System.Drawing.StringFormat
    $sf.Alignment = [System.Drawing.StringAlignment]::Center
    $sf.LineAlignment = [System.Drawing.StringAlignment]::Center

    $rect = New-Object System.Drawing.RectangleF(0, 0, $size, $size)
    $g.DrawString("P", $font, $whiteBrush, $rect, $sf)

    $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()

    Write-Host "Created: $path"
}

# Create PNG icons
Create-SimpleIcon 32 (Join-Path $iconsDir "32x32.png")
Create-SimpleIcon 128 (Join-Path $iconsDir "128x128.png")
Create-SimpleIcon 256 (Join-Path $iconsDir "128x128@2x.png")

# Create ICO file (simple approach - copy 32x32 as ico)
$bmp32 = New-Object System.Drawing.Bitmap((Join-Path $iconsDir "32x32.png"))
$icon = [System.Drawing.Icon]::FromHandle($bmp32.GetHicon())
$fs = New-Object System.IO.FileStream((Join-Path $iconsDir "icon.ico"), [System.IO.FileMode]::Create)
$icon.Save($fs)
$fs.Close()
$bmp32.Dispose()

Write-Host "Created: icon.ico"
Write-Host "Icons created successfully!"

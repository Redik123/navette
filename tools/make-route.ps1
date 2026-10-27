# Génère route.geojson : boucle routière passant par les arrêts donnés, dans
# l'ordre, puis revenant au premier. Utilise le serveur de démo OSRM (voiture).
#
#   .\tools\make-route.ps1 -Stops "46.22772,7.35838", "46.22374,7.37032"
#
# Pour forcer un passage par une rue précise, ajouter un point intermédiaire
# dans -Stops (ce n'est pas obligatoirement un arrêt de la navette).

param(
    [Parameter(Mandatory)] [string[]] $Stops,
    [string] $Out = (Join-Path $PSScriptRoot "..\route.geojson")
)

$ErrorActionPreference = "Stop"
$invariant = [System.Globalization.CultureInfo]::InvariantCulture

$points = foreach ($s in $Stops + $Stops[0]) {
    $lat, $lon = $s.Split(",") | ForEach-Object { [double]::Parse($_.Trim(), $invariant) }
    "{0},{1}" -f $lon.ToString($invariant), $lat.ToString($invariant)
}

# continue_straight=false : le bus peut faire demi-tour à un arrêt au lieu de faire le tour du quartier.
$url = "https://router.project-osrm.org/route/v1/driving/{0}?overview=full&geometries=geojson&continue_straight=false" -f ($points -join ";")
$res = Invoke-RestMethod -Uri $url
if ($res.code -ne "Ok") { throw "OSRM : $($res.code) $($res.message)" }

$route = $res.routes[0]
$coords = $route.geometry.coordinates | ForEach-Object {
    , @([math]::Round($_[0], 6), [math]::Round($_[1], 6))
}

$geojson = [ordered]@{
    type     = "FeatureCollection"
    features = @(
        [ordered]@{
            type       = "Feature"
            properties = [ordered]@{
                name       = "Boucle navette"
                distance_m = [math]::Round($route.distance)
                source     = "OSRM"
            }
            geometry   = [ordered]@{ type = "LineString"; coordinates = $coords }
        }
    )
}

$json = $geojson | ConvertTo-Json -Depth 6 -Compress
[System.IO.File]::WriteAllText([System.IO.Path]::GetFullPath($Out), $json)  # UTF-8 sans BOM
"Trace ecrit dans {0} : {1:N0} m, {2} points" -f (Resolve-Path $Out), $route.distance, $coords.Count

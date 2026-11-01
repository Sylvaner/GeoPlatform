# Geo Data Platform

Application de cartographie locale et d’exports géographiques. Le dépôt contient
le serveur HTTP, l’interface web, les commandes CLI et les moteurs Rust de
téléchargement et de génération. Il se compile sans le dépôt historique ni ses
exécutables.

## Fonctionnalités

- Téléchargement par zone des couches relief, orthophoto, LiDAR HD, bâtiments,
  transports, cadastre et hydrographie.
- Exports Blender, STL continu et STL en strates.
- Aperçu interactif dans une fenêtre modale pour les STL ; l’historique purge les
  exports dont le fichier final a été supprimé.
- Interface cartographique, API, files de jobs, annulation, historique et cache.
- Commandes `geo-data`, `geo-export`, `geo-stl` et `geo-stl-layers`, partageant
  la configuration et le stockage du serveur.

## Prérequis

- Rust 1.88 ou plus récent et Cargo. Une connexion Internet est nécessaire au
  premier build pour télécharger les crates.
- Une connexion aux services géographiques lors des téléchargements : IGN
  Géoplateforme WMTS/WFS (`data.geopf.fr`) et catalogue LiDAR, ainsi que Sandre
  BD TOPAGE (`services.sandre.eaufrance.fr`). Disponibilité, couverture, quotas
  et formats relèvent de ces services externes.
- Blender installé et détectable via `PATH` ou l’installation Windows standard
  pour produire des fichiers `.blend`. Les téléchargements et exports STL ne
  nécessitent pas Blender.
- L’interface récupère Leaflet sur `unpkg.com` et les tuiles de fond auprès
  d’OpenStreetMap ; l’affichage du fond de carte nécessite un accès Internet.
  Three.js est embarqué localement.

Aucune bibliothèque GIS native (GDAL/PROJ/SQLite) n’est requise. Le lecteur COPC
est fourni sous `engine/vendor/copc-reader` et son correctif Cargo est configuré
dans le workspace.

## Installation et lancement

Depuis un clone du dépôt, placez-vous dans ce répertoire (`new`) :

```powershell
cd chemin\vers\geo-data
cargo build --workspace --bins
cargo run --bin geo-server -- --config geo.toml
```

Sous Windows, `compiler-tous.ps1` compile les exécutables en release puis les
copie à la racine de `new` :

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\compiler-tous.ps1
```

Les exécutables release sont alors copiés à la racine (`geo-server.exe`,
`geo-data.exe`, `geo-export.exe`, `geo-stl.exe` et `geo-stl-layers.exe`).
Exemple : `.\geo-server.exe --config geo.toml`.

Sous Windows, `demarrer-serveur.cmd` démarre le serveur en arrière-plan et
`arreter-serveur.cmd` l’arrête. Le serveur écoute par défaut sur
<http://127.0.0.1:8080/>. Vérifiez son état :

```powershell
Invoke-RestMethod http://127.0.0.1:8080/api/v1/health
```

`Ctrl+C` arrête proprement un serveur lancé au premier plan. La configuration
fournie utilise `127.0.0.1` : l’API n’a pas d’authentification et permet de
lancer des traitements, modifier la configuration et supprimer des fichiers.
Ne l’exposez pas à un réseau non fiable et ne remplacez pas l’adresse par
`0.0.0.0` sans ajouter une protection adaptée.

## Configuration et stockage

`geo.toml` est propre à chaque machine. `storage.root` peut être un chemin
relatif au fichier de configuration ou un chemin absolu. Aucun secret n’est
requis ni stocké dans ce fichier.

Pour migrer une ancienne configuration de `new`, supprimez sa section
`[engine]` et la clé `legacy_root` : les moteurs sont maintenant inclus. Les
anciens chemins absolus de `[storage].root` restent acceptés ; vous pouvez les
garder ou les remplacer par un chemin relatif comme `storage`.

```text
storage/    # créé au premier démarrage
  cache/results/    résultats réutilisables
  data/             tuiles, pages WFS et dalles géographiques
  exports/<job>/    fichiers produits et configurations d’export
  jobs/<job>/       manifestes et journaux de téléchargement
  state/jobs/       état persistant des jobs
```

Le serveur et les CLI lisent le même `geo.toml` et écrivent dans le même
stockage. Depuis un autre répertoire, passez `--config` avec le chemin du fichier.

Le worker de cache se règle dans `[cache_warmup]` :

```toml
[cache_warmup]
stl_grid_steps_m = [50.0, 80.0, 100.0]
blender_atlas_resolutions_px = [512, 2048, 4096, 8192]
```

Le worker est sérialisé et rescane les zones terminées au démarrage, puis après
un téléchargement. Il préchauffe les STL uniquement pour les zones LiDAR, et les
atlas Blender uniquement si la zone contient l’orthophoto et une source de
terrain. Retirer une valeur arrête son préchauffage ; les fichiers existants
restent réutilisables à la demande et sont soumis aux limites de conservation.

Chaque pas STL est actuellement calculé par une lecture LiDAR distincte. Le
préchauffage déplace donc le coût vers l’arrière-plan et réduit l’attente du
premier export correspondant, mais ne réduit pas le travail total si toutes les
variantes sont précalculées. Videz `stl_grid_steps_m` si ce compromis ne convient
pas.

Les atlas Blender réutilisent une scène de base commune ; chaque résolution
requiert encore une conversion des images et un `.blend` final. Le worker ne se
lance que si Blender est installé. La configuration fournie active les quatre
résolutions demandées. Un atlas 8192 px peut demander environ 1 Gio pour son
seul buffer de pixels ; retirez `8192` de `blender_atlas_resolutions_px` si la
machine ne dispose pas de cette marge. Le cache final conserve au plus six
variantes récentes et le cache de base douze.

Les clés STL intègrent les paramètres de grille, la qualité, les fichiers LiDAR,
leurs tailles et dates : une source modifiée produit une nouvelle clé. Les clés
Blender incluent les chemins, tailles et dates des fichiers sous
`storage.root/data`, et un téléchargement terminé invalide les index de
résultats d’export API. Ces empreintes utilisent les métadonnées, pas un hash
complet de chaque fichier.

La surface d’eau suit les mêmes triangles et le même relief interpolé que le
terrain/orthophoto, avec une garde de 15 cm ; une extrusion Blender de 1 m vers
le bas lui donne un volume fermé. Les routes utilisent un ray-cast séparé sur le
terrain. Les distances de clipping caméra et viewport Blender valent 10 000 m
par défaut.

## Commandes

Télécharger LiDAR et bâtiments sur une zone carrée de 3 km (centre au format
latitude, longitude) :

```powershell
cargo run --bin geo-data -- --center 48.26183 4.73872 --size 3000 --layers lidar,buildings
```

Créer un STL continu :

```powershell
cargo run --bin geo-stl -- --center 48.26183 4.73872 --size 3000 --quality standard --width-mm 150
```

Créer un STL en strates d’un mètre réel :

```powershell
cargo run --bin geo-stl-layers -- --center 48.26183 4.73872 --size 3000 --quality standard --width-mm 150 --layer-height-m 1
```

Créer une scène Blender (Blender doit être installé) :

```powershell
cargo run --bin geo-export -- --center 48.26183 4.73872 --size 3000 --format blender --quality standard
```

Les options disponibles sont affichées par `cargo run --bin <nom> -- --help`.
`geo-export` accepte `--format blender`, `stl` ou `stl_layers` ; sans cette
option, il choisit Blender. Les binaires `geo-stl` et `geo-stl-layers` fixent
leur format et refusent un format explicitement différent. Toutes les commandes
renvoient le job et ses fichiers en JSON. Elles utilisent le même cache que l’API.

## API

Les opérations longues renvoient un job ; suivez-le avec
`GET /api/v1/jobs/{id}`. Exemple de téléchargement :

```powershell
$body = @{
  area = @{ center = @(48.26183, 4.73872); size_m = 3000 }
  layers = @("lidar", "buildings")
} | ConvertTo-Json -Depth 5

Invoke-RestMethod -Method Post `
  -Uri http://127.0.0.1:8080/api/v1/downloads `
  -ContentType application/json -Body $body
```

Pour un export, `POST /api/v1/exports` accepte `area`, `format`, `quality`,
`layers` et les options dans `advanced`. Les valeurs de `format` sont `blender`,
`stl` et `stl_layers`. La documentation OpenAPI est disponible sur
`/api/v1/openapi.json`.

Routes utiles : `GET /api/v1/health`, `GET /api/v1/layers`,
`POST /api/v1/downloads`, `POST /api/v1/exports`, `GET /api/v1/jobs`,
`POST /api/v1/jobs/{id}/cancel`, `GET /api/v1/files`,
`GET /api/v1/files/{id}/content`, `GET /api/v1/map` et
`GET /api/v1/dashboard`. L’API fournit également la gestion des zones, des
profils d’export, des journaux et de la configuration.

## Tests et développement

```powershell
cargo build --workspace --bins
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Le test d’export Blender de bout en bout est ignoré par défaut ; il nécessite une
installation Blender. Les autres tests utilisent des fixtures locales et ne
téléchargent pas de données réelles.

## Publication

Aucune licence n’est déclarée pour le projet. Avant toute publication publique,
vérifiez les droits et la provenance des sources historiques intégrées sous
`engine/`, du script `blender/postprocess_scene.py` et des ressources web. Le
manifest `engine/vendor/copc-reader/Cargo.toml` indique `MIT OR Apache-2.0` pour
ce crate ; vérifiez également la licence et l’avis de copyright des fichiers
embarqués. N’ajoutez pas de licence au projet sans décision explicite.

# Navette · Ragnarok Festival

**https://navette.ragnarok-festival.ch**

Page publique qui montre **où est la navette en ce moment**, son **tracé** et le
**temps d'attente à chaque arrêt**, mise à jour en direct. Un téléphone allumé
dans le bus envoie sa position GPS à un petit serveur Rust, qui la diffuse
à tous les festivaliers connectés.

```
 Téléphone du bus ──(GPS toutes les 3-5 s, HTTPS)──▶  Serveur Rust (axum)
 Traccar Client ou /driver                            · accroche la position au tracé
 Chauffeur ──▶ /driver (à bord, message, pause)      · calcule statut + ETA
 Orga ──▶ /admin (idem + tableau de bord)             · gère les signes sur le trajet
                                                      · diffuse en SSE
                                                            │
       Festivaliers ◀── page / (Leaflet, « Faire signe ») ──┘
```

Toute la logique est en Rust : accrochage au tracé (même quand l'aller et le
retour empruntent la même route), sens de marche, détection des arrêts, temps
d'arrivée, « signal perdu », prochains trains en gare. La page web ne fait
qu'afficher.

Sur la page publique :

- **Prochains trains depuis Sion**, avec retards et changements de voie en temps
  réel (API publique transport.opendata.ch). Le serveur garde la réponse en
  cache : une requête par minute au plus, quel que soit le nombre de visiteurs,
  et aucune quand personne ne regarde.
- **« Me localiser »** : point bleu sur la carte, arrêt le plus proche, distance
  et temps à pied. Le calcul se fait dans le navigateur : la position du
  visiteur n'est envoyée nulle part. Le GPS se coupe quand l'onglet passe en
  arrière-plan.
- **Places libres** sur les 16, et l'état de la navette : « À quai, part dès
  qu'elle est pleine », « Départ », « En route ».
- **« Faire signe à la navette »** pour qui est sur le trajet, loin d'un arrêt
  (voir plus bas).
- **« Aide »** : une personne en danger près du trajet (moins de 2 km) envoie
  sa position exacte au chauffeur et à l'orga, qui essaient de venir. Elle
  n'est jamais diffusée au public. La fenêtre propose d'abord d'appeler le 117
  (police) ou le 144 (ambulance) : l'équipe vient en plus, pas à la place. Sur
  les pages chauffeur et orga, l'alerte s'affiche en rouge tout en haut et
  sonne toutes les 6 s jusqu'à ce que quelqu'un réponde « J'y vais » ; la
  personne voit alors que quelqu'un arrive. Alerte gardée en mémoire seulement
  (un redémarrage l'efface) et close au bout de 2 h.

**Tenue en charge** (mesurée en local) : 1000 spectateurs simultanés, 0 échec,
~48 Mo de RAM, 0,14 s de CPU en 15 s. Les 200 personnes prévues en
permanence ne pèsent rien. Le fond de carte (swisstopo) est servi par la
Confédération, pas par notre serveur.

## Pages

| Adresse | Pour qui |
|---|---|
| `/` | Le public : carte, tracé, navette en direct, places libres, temps d'attente, « Faire signe », prochains trains, « Me localiser » |
| `/driver` | Le chauffeur : petite carte (navette, signes, demandes d'aide), personnes à bord, « On part », signes sur le trajet, message aux festivaliers, auto / pause / hors service (et le GPS si Traccar Client n'est pas utilisé) |
| `/admin` | L'orga : les mêmes commandes, plus un tableau de bord (statut public, âge de la dernière position, batterie du téléphone, spectateurs connectés) et l'**éditeur de trajet** |

**Route barrée ?** Dans `/admin`, section « Trajet » : les points blancs
sont les points de passage de l'aller (le retour reprend le même chemin). On
les glisse, on en ajoute en touchant la carte, on en retire en les touchant ;
« Calculer » montre le nouveau trajet en jaune (calculé sur les vraies routes
par le serveur public OSRM), « Appliquer » le publie : toutes les pages
ouvertes se rechargent avec le nouveau tracé. Il est gardé dans
`data/route.json` (volume `navette-data`), donc il survit aux redémarrages et
aux mises à jour. « Trajet d'origine » revient à `route.geojson`. Garde-fous :
chaque arrêt doit rester sur le trajet, et un trajet de plus de trois fois la
longueur d'origine est refusé. Les signes « faire signe » en cours sont clos
au changement de tracé.

## Pas d'horaire : comment ça marche

La navette a **16 places**. Elle part **dès qu'elle est pleine**, ou quand le
chauffeur (ou l'orga) le décide.

- **Personnes à bord** : boutons − / + (et « Vide » / « Plein ») sur
  `/driver` ou `/admin`. Le compteur **repart de zéro tout seul à
  l'arrivée à un arrêt** (tout le monde descend). Le GPS qui sautille au bord
  de l'arrêt ne le remet pas à zéro : il faut vraiment être reparti. Tant que
  personne n'a touché au compteur, la page publique n'affiche pas de places.
- **« On part »** : à quai, la page publique affiche « part dès qu'elle est
  pleine » et les temps vers les autres arrêts « après le départ ». Un appui
  sur « On part » passe la page en « Départ », puis « En route » dès que la
  navette roule.
- **Signes sur le trajet** : quelqu'un qui ne peut vraiment pas rejoindre un
  arrêt appuie sur « Faire signe à la navette », choisit sa direction et
  indique combien ils sont. Le serveur vérifie :
  - qu'il est **sur le trajet** (50 m max de la route) ;
  - qu'il n'est **pas à moins de 150 m d'un arrêt** (sinon : « attends à l'arrêt ») ;
  - que la navette circule, et que le groupe fait 6 personnes au plus.

  Le chauffeur voit la demande sur sa page (bip et vibration), avec la
  distance devant lui, et peut la marquer **Pris** (les personnes s'ajoutent
  au compteur) ou **Ignorer**. Elle disparaît d'elle-même **quand la navette
  est passée** par ce point, ou après 20 minutes. La personne suit tout ça sur
  sa page : temps avant le passage, puis « pris en charge », « passée »…

  Confidentialité : le serveur ne garde que **le point de la route** où la
  personne attend, jamais sa position GPS, et l'efface dès que la demande est
  close. Le public ne voit les demandes de personne d'autre.

  Contre les abus : une demande à la fois par téléphone, 3 au plus par
  adresse IP (plusieurs téléphones partagent souvent une IP), 6 créations max
  par IP en 10 minutes, 12 demandes en cours au total. Réglages dans
  `[pickups]` de `config.toml`.

## Tester en local

Prérequis : Rust (`rustup`). Les jetons secrets sont dans `.env` (copier
`.env.example`, ne jamais publier ce fichier : il est exclu du dépôt).

```bash
.\tools\dev.ps1 --simulate
```

Ouvrir http://localhost:8080 : une navette fictive roule sur le tracé.
Sans `--simulate`, le serveur attend les vraies positions.

Tests unitaires : `cargo test`.

## À compléter avant le festival

1. **Heures de service** de la navette (`[event] schedule`).
2. **Vitesse moyenne** (`tracking.avg_speed_kmh`) : la mesurer lors d'un tour
   d'essai (durée de la boucle ÷ 4,6 km) pour des temps d'attente justes.

### Le tracé

Le tracé actuel (4,6 km) suit la route réelle du chauffeur : départ côté sud
de la gare, descente vers Sous-Gare, Route des Ronquoz, Rue de la Dixence,
puis l'accès jusqu'à l'entrée de l'Usine, et retour par le même chemin. Il a
été généré ainsi (arrêt gare, points de passage, arrêt Usine, puis les mêmes
points de passage dans l'autre sens) :

```bash
.\tools\make-route.ps1 -Stops "46.226843,7.358991", "46.225033,7.359238", "46.224571,7.361794", "46.226963,7.366379", "46.224831,7.37386", "46.224075,7.373849", "46.223239,7.374629", "46.224075,7.373849", "46.224831,7.37386", "46.226963,7.366379", "46.224571,7.361794", "46.225033,7.359238"
```

Les points sont visités dans l'ordre puis le tracé revient au premier
(boucle). Pour forcer une rue, ajouter un point intermédiaire. Si un arrêt
bouge, mettre à jour `[[stops]]` dans `config.toml` **et** régénérer le tracé.
On peut aussi dessiner/corriger le tracé à la main sur https://geojson.io et
remplacer `route.geojson` (une seule `LineString`, en boucle).

## Le téléphone du bus

### Option A — Traccar Client (recommandé)

Application gratuite (Android / iOS) qui continue d'envoyer **écran éteint et
en arrière-plan**, et garde les positions en mémoire en cas de coupure réseau.

Réglages :

| Champ | Valeur |
|---|---|
| URL du serveur | `https://navette.ragnarok-festival.ch/api/osmand` |
| Identifiant de l'appareil | la valeur de `NAVETTE_DRIVER_TOKEN` (dans `.env`) |
| Précision | Haute |
| Fréquence / intervalle | 5 s |
| Distance | 0 |
| Mise en tampon hors ligne | activée |

Sur Android : désactiver l'optimisation de batterie pour Traccar Client et
autoriser la localisation « tout le temps ».

### Option B — page `/driver`

Ouvrir `https://navette.ragnarok-festival.ch/driver#t=<NAVETTE_DRIVER_TOKEN>` sur le
téléphone, puis **Démarrer**. La page garde l'écran allumé, mais doit rester
au premier plan : si on change d'application, les envois s'arrêtent.

Dans les deux cas : **téléphone branché en permanence** dans le bus, et un
support qui laisse voir le ciel (pare-brise).

Le compteur de places, « On part » et les signes sont sur `/driver`
**dans les deux cas** : avec Traccar Client, on ouvre simplement la page sans
appuyer sur Démarrer (elle indique que la position arrive bien par Traccar).
À manipuler à l'arrêt, jamais en roulant.

## Déploiement (Docker / Portainer)

Adresse finale : **https://navette.ragnarok-festival.ch**. Le GPS d'un
téléphone n'est accessible qu'en **HTTPS**.

À chaque push sur `main`, GitHub Actions lance les tests puis publie l'image
**`ghcr.io/redik123/navette:latest`** (onglet *Actions* du dépôt). Le serveur
ne compile rien : il télécharge l'image. Elle contient déjà `config.toml`,
`route.geojson` et les pages web ; seuls les jetons sont à fournir.

**Jetons** : deux chaînes aléatoires de 16 caractères minimum, sans `+` ni
`/` (elles passent dans des liens). Sur le serveur : `openssl rand -hex 24`.

**DNS** : un enregistrement `A` de `navette.ragnarok-festival.ch` vers l'IP du
serveur.

### Avec Portainer

**Le plus simple** (serveur avec sa propre IP, ports 80/443 libres, comme
`navette.jcloud.ik-server.com`) : *Stacks* → *Add stack* → **Web editor**,
coller le contenu de [`stack-portainer.yml`](stack-portainer.yml), renseigner
`NAVETTE_DRIVER_TOKEN` et `NAVETTE_ADMIN_TOKEN` dans *Environment variables*,
puis *Deploy the stack*. La navette, Caddy (HTTPS automatique) et Watchtower
(mises à jour automatiques) démarrent ensemble.

**Avec un dépôt Git et un proxy existant** :

1. *Stacks* → *Add stack* → **Repository**.
2. URL : `https://github.com/Redik123/navette`, référence `refs/heads/main`,
   fichier : `docker-compose.yml`.
3. *Environment variables* : `NAVETTE_DRIVER_TOKEN` et `NAVETTE_ADMIN_TOKEN`
   (et `NAVETTE_PORT` si 8080 est déjà pris).
4. **HTTPS**, au choix :
   - **un proxy existe déjà** (Nginx Proxy Manager, Traefik…) : créer l'hôte
     `navette.ragnarok-festival.ch` → `http://<ip-du-serveur>:8080` (ou
     `http://navette:8080` si le proxy est sur le même réseau Docker), avec
     certificat Let's Encrypt et HTTPS forcé. Rien d'autre à régler : le
     serveur demande lui-même aux proxys de ne pas retenir le flux temps réel ;
   - **pas de proxy** : ajouter `docker-compose.caddy.yml` dans *Additional
     paths*. Caddy prend les ports 80 et 443 et obtient le certificat tout seul.
5. *Deploy the stack*. Le conteneur passe en **healthy** quand il répond.

**Mises à jour** : avec `stack-portainer.yml`, c'est automatique. GitHub
reconstruit l'image en 2 à 4 minutes, Watchtower la repère dans les 2 minutes
et redémarre la navette (quelques secondes de coupure, les pages se
reconnectent seules). Pour l'installer tout de suite : *Update the stack* avec
**Re-pull image**. Pour figer la version (par exemple pendant le festival) :
arrêter le conteneur `watchtower` dans Portainer.

Un redémarrage garde la pause et le message (volume `navette-data`), mais remet
à zéro le compteur de passagers et efface les demandes « faire signe » en
cours : éviter de pousser pendant le service.

Si Portainer n'arrive pas à télécharger l'image, c'est que le paquet GitHub
est privé : *github.com/Redik123?tab=packages* → **navette** → *Package
settings* → *Change visibility* → **Public** (ou ajouter un registre ghcr.io
avec un jeton GitHub dans Portainer).

### En ligne de commande

```bash
git clone https://github.com/Redik123/navette.git && cd navette
cp .env.example .env    # puis y mettre les jetons
docker compose up -d                                                   # derrière un proxy existant
docker compose -f docker-compose.yml -f docker-compose.caddy.yml up -d # avec Caddy (HTTPS auto)
```

Pour construire l'image sur place au lieu de la télécharger :
`docker build -t ghcr.io/redik123/navette:latest .` avant `docker compose up -d`.

### Modifier la configuration

Arrêts, tracé, horaires, textes : modifier `config.toml`, `route.geojson` ou
`static/` dans le dépôt et pousser sur `main` : le serveur se met à jour tout
seul quelques minutes plus tard. Pour un essai rapide sans passer par GitHub, on peut aussi
monter un fichier par-dessus celui de l'image
(`/chemin/config.toml:/app/config.toml:ro` dans `volumes`).

### Sans Docker

`cargo build --release`, puis lancer `target/release/navette config.toml`
derrière n'importe quel proxy HTTPS.

## Checklist jour J

- [ ] Tour d'essai complet avec le vrai téléphone, la page publique ouverte à côté
- [ ] `/admin` : « Dernière position » < 10 s, batterie qui remonte
- [ ] Téléphone branché, appli autorisée en arrière-plan, son coupé
- [ ] Un téléphone de secours configuré pareil (même identifiant), éteint, dans la boîte à gants
- [ ] `/driver` ouvert sur le téléphone du chauffeur : compteur à 0, un essai de « On part »
- [ ] Un essai de « Faire signe » sur le trajet avec un téléphone de l'orga, puis « Ignorer »
- [ ] Mode **Auto** (page chauffeur ou admin) à l'ouverture, **Hors service** à la fermeture (efface aussi les signes en cours)
- [ ] QR code vers la page affiché à la gare, à l'entrée, aux bars

## API

| Méthode | Chemin | Rôle |
|---|---|---|
| GET | `/api/config` | Tracé, arrêts, horaires |
| GET | `/api/state` | État courant (JSON) |
| GET | `/api/stream` | Flux temps réel (Server-Sent Events) |
| GET | `/api/trains` | Prochains départs en gare (cache serveur, `[trains]` dans `config.toml`) |
| POST | `/api/driver/position` | Position (JSON, `Authorization: Bearer <driver>`) |
| GET/POST | `/api/osmand` | Position au protocole OsmAnd (Traccar Client) |
| POST | `/api/pickups` | Faire signe : `{"lat", "lon", "accuracy", "count", "dest", "device"}` → `{"id", "token", "lat", "lon"}` ou `422 {"error"}` |
| POST | `/api/pickups/cancel` | `{"id", "token"}` |
| POST | `/api/help` | Demande d'aide : `{"lat", "lon", "accuracy", "note", "device"}` → `{"id", "token"}` ou `422 {"error"}` |
| POST | `/api/help/status` | `{"id", "token"}` → `{"state": "open" \| "seen" \| "closed"}` |
| POST | `/api/help/cancel` | `{"id", "token"}` |
| POST | `/api/crew/help` | `{"id": "...", "action": "seen" \| "closed"}` |
| GET | `/api/admin/route` | Points de passage courants et d'origine (`Bearer <admin>`) |
| POST | `/api/admin/route/preview` | `{"via": [[lat, lon], …]}` → `{"route", "length_m", "problem"}` (rien n'est changé) |
| POST | `/api/admin/route` | `{"via": […]}` : recalcule et publie le trajet |
| POST | `/api/admin/route/reset` | Revient à `route.geojson` |
| GET | `/api/crew/state` | État + dernière position brute + signes en cours + spectateurs (`Bearer <driver>` ou `<admin>`) |
| POST | `/api/crew/passengers` | `{"passengers": 9}` |
| POST | `/api/crew/depart` | Annonce le départ |
| POST | `/api/crew/pickup` | `{"id": "...", "action": "picked" \| "dismissed"}` |
| POST | `/api/crew/service` | `{"mode": "auto" \| "pause" \| "off"}` |
| POST | `/api/crew/message` | `{"text": "...", "level": "info" \| "warn" \| "alert"}` (texte vide = effacer) |

## Code

| Fichier | Contenu |
|---|---|
| `src/geo.rs` | Projection, accrochage au tracé, distances le long de la boucle |
| `src/tracker.rs` | Filtrage des positions, choix aller/retour, statut, ETA, places, départ |
| `src/tracker/pickups.rs` | « Faire signe » : vérifications, sens, passage, expiration, anti-abus |
| `src/state.rs` | État partagé, diffusion, persistance (`data/state.json`) |
| `src/api.rs` | Routes HTTP, formats Traccar, authentification |
| `src/trains.rs` | Prochains trains (transport.opendata.ch), mis en cache |
| `src/sim.rs` | Navette fictive (`--simulate`) |
| `static/` | Pages web (Leaflet) ; `crew.js` : commandes communes chauffeur / orga |
| `Dockerfile`, `docker-compose*.yml` | Image et stacks Docker / Portainer |
| `.github/workflows/docker.yml` | Tests et publication de l'image sur ghcr.io |

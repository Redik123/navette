# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- **Festivaliers** du Ragnarok Festival 2026 · Halloween Édition (30 et 31 octobre 2026, Usine de Chandoline, Sion). Public électro / techno, de nuit (service de 21:00 à 01:30, puis de 04:20 à 06:30), téléphone en main, à la gare de Sion (côté sud) ou devant l'Usine. Ils veulent savoir en un coup d'œil où est la navette, si elle est à quai, combien de places restent et quand elle arrive. Environ 200 personnes connectées en même temps.
- **Le chauffeur**, bénévole, à bord : compte les personnes, annonce le départ, voit les signes sur le trajet, publie un message, passe en pause.
- **L'orga** : mêmes commandes que le chauffeur, plus un tableau de bord.

## Product Purpose

Montrer en direct la position et le trajet de la navette gratuite de 16 places entre la gare de Sion et l'Usine de Chandoline. Pas d'horaire : elle part dès qu'elle est pleine, ou quand le chauffeur décide. Réussite : personne n'attend dans le froid sans savoir, et personne ne se fait arnaquer par une fausse navette payante.

## Positioning

Un suivi fait pour une seule navette et un seul trajet, de nuit : position GPS réelle accrochée au tracé, places libres annoncées par le chauffeur, « faire signe » sur la route quand on ne peut pas rejoindre un arrêt, prochains trains CFF en gare.

## Operating Context

- Page publique : `navette.ragnarok-festival.ch`, ouverte depuis un lien ou un QR code, sur téléphone surtout, la nuit, souvent dehors.
- Téléphone du bus : Traccar Client ou `/driver`. Orga : `/admin`.
- Hébergement Docker / Portainer, mises à jour automatiques (Watchtower) à chaque push sur `main`.

## Capabilities and Constraints

- Carte Leaflet avec fond swisstopo, tracé en boucle (4,6 km), deux arrêts : « Gare CFF de Sion » (côté sud de la gare) et « Usine de Chandoline » (entrée du festival).
- Statuts : en route, à quai (part dès qu'elle est pleine), départ, signal perdu, hors ligne, pause, hors service.
- Places libres sur 16, temps d'arrivée à chaque arrêt, « Me localiser » (rien n'est envoyé), « Faire signe » (dernier recours, uniquement sur le tracé), prochains trains depuis Sion, message de l'orga ou du chauffeur en bandeau.
- Mention obligatoire : navette gratuite, conduite par un bénévole ; quelqu'un qui fait payer en se présentant comme la navette du Ragnarok est un arnaqueur.
- Serveur Rust (axum), pages statiques sans framework ni étape de build.

## Brand Commitments

- Nom : Ragnarok Festival 2026 · Halloween Édition.
- Logo « RAGNAROK » en police **Norse** (fichiers `static/fonts/`, repris de ragnarok-events.ch). C'est le seul élément d'identité imposé : les couleurs et le reste du style sont libres.
- Le propriétaire trouve que le look actuel « fait IA » : fond sombre avec néons violets, et rien qui évoque le festival, Halloween ou l'Usine. À ne pas reproduire.

## Evidence on Hand

- Police Norse (`static/fonts/Norse.otf`, `static/fonts/Norsebold.otf`).
- Aucune photo, affiche ni line-up officiels fournis : ne pas en inventer.

## Product Principles

- L'information d'abord : où est la navette et quand elle arrive, lisible en une seconde, de nuit, en marchant.
- Dire la vérité sur l'état : signal perdu, hors ligne ou pause ne doivent jamais ressembler à « en route ».
- Protéger le chauffeur et le public : gratuité et bénévolat visibles, « faire signe » présenté comme un dernier recours.
- Une page légère : ça doit marcher sur un vieux téléphone avec un réseau saturé autour du festival.

## Accessibility & Inclusion

Lecture dehors, de nuit, parfois après quelques verres : gros contrastes, cibles tactiles généreuses, textes courts.

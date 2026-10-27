'use strict';

// Toute la logique (accrochage au tracé, ETA, statut) est calculée par le
// serveur Rust : cette page ne fait qu'afficher ce qu'il diffuse.

const STATUS = {
  moving: { pill: 'En route', cls: 'live' },
  at_stop: { pill: 'À quai', cls: 'stop' },
  departing: { pill: 'Départ', cls: 'live' },
  stale: { pill: 'Signal perdu', cls: 'warn' },
  offline: { pill: 'Hors ligne', cls: 'off' },
  pause: { pill: 'Pause', cls: 'warn' },
  off: { pill: 'Hors service', cls: 'off' },
};

/** Statuts où la position et les temps d'attente sont fiables. */
const LIVE = new Set(['moving', 'at_stop', 'departing']);

const PICKUP_KEY = 'navette-pickup';
const DEVICE_KEY = 'navette-device';

const PICKUP_OUTCOMES = {
  picked: 'Le chauffeur t’a pris en charge. Bon trajet !',
  passed: 'La navette est passée par ton point.',
  dismissed: 'Le chauffeur n’a pas pu s’arrêter. Rejoins un arrêt ou refais signe.',
  expired: 'Demande expirée : personne n’est passé à temps. Refais signe si besoin.',
  cancelled: 'Demande annulée.',
};

const BUS_SVG = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 6v6M15 6v6M2 12h19.6M18 18h3s.5-1.7.8-2.8c.1-.4.2-.8.2-1.2 0-.4-.1-.8-.2-1.2l-1.4-5C20.1 6.8 19.1 6 18 6H4a2 2 0 0 0-2 2v10h3"/><circle cx="7" cy="18" r="2"/><path d="M9 18h5"/><circle cx="16" cy="18" r="2"/></svg>';

const REFRESH_MS = 30_000;
const TRAINS_REFRESH_MS = 60_000;

// Marche à pied : 4,5 km/h, et les rues ne vont pas en ligne droite.
const WALK_SPEED = 1.25;
const WALK_DETOUR = 1.3;

const GEO_ERRORS = {
  1: 'Localisation refusée. Autorise-la pour ce site dans les réglages du navigateur.',
  2: 'Position introuvable pour le moment. Réessaie un peu plus tard.',
  3: 'La localisation prend trop de temps. Réessaie dans un instant.',
};

// Heures de Suisse, même sur un téléphone réglé sur un autre fuseau.
const TIME_FMT = new Intl.DateTimeFormat('fr-CH', { hour: '2-digit', minute: '2-digit', timeZone: 'Europe/Zurich' });

const $ = (id) => document.getElementById(id);

const app = {
  cfg: null,
  snap: null,
  stops: new Map(),
  clockOffset: 0, // horloge serveur - horloge locale
  follow: false,
  autoFit: true,
  map: null,
  routeBounds: null,
  busMarker: null,
  anim: 0,
  es: null,
  trains: null,
  trainsKey: '',
  locate: { on: false, watch: null, me: null, framed: false, layers: null },
  pickup: null, // ma demande « faire signe » : { id, token, lat, lon, count, dest }
  pickupMarker: null,
  pickupCount: 1,
  pickupDest: null,
};

const serverNow = () => Date.now() + app.clockOffset;

function fmtMinutes(s) {
  return s < 60 ? '< 1 min' : `${Math.round(s / 60)} min`;
}

function fmtCountdown(s) {
  if (s < 60) return '< 1 min';
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')}`;
}

function fmtDistance(m) {
  return m < 1000 ? `${Math.round(m / 10) * 10} m` : `${(m / 1000).toFixed(1).replace('.', ',')} km`;
}

function fmtAgo(s) {
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  return `${Math.floor(s / 3600)} h`;
}

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

async function fetchJson(url) {
  const res = await fetch(url, { cache: 'no-store' });
  if (!res.ok) throw new Error(`${url} : ${res.status}`);
  return res.json();
}

// Le réseau d'un festival est capricieux : on réessaie jusqu'à y arriver.
async function fetchJsonRetry(url) {
  for (let delay = 1000; ; delay = Math.min(delay * 2, 15_000)) {
    try {
      return await fetchJson(url);
    } catch {
      setConn('retry');
      await new Promise((r) => setTimeout(r, delay));
    }
  }
}

// ------------------------------------------------------------------ carte

function setupMap(cfg) {
  const map = L.map('map', { zoomControl: false, attributionControl: false });
  app.map = map;
  L.control.zoom({ position: 'bottomright' }).addTo(map);
  L.control.attribution({ position: 'topright', prefix: '<a href="https://leafletjs.com">Leaflet</a>' }).addTo(map);
  // Carte nationale swisstopo (gratuite, sans clé), assombrie en CSS.
  L.tileLayer('https://wmts.geo.admin.ch/1.0.0/ch.swisstopo.pixelkarte-grau/default/current/3857/{z}/{x}/{y}.jpeg', {
    minZoom: 12,
    maxNativeZoom: 18,
    maxZoom: 19,
    attribution: '&copy; <a href="https://www.swisstopo.admin.ch/">swisstopo</a>',
  }).addTo(map);

  const line = { lineCap: 'round', lineJoin: 'round', interactive: false };
  L.polyline(cfg.route, { ...line, color: '#7c3aed', weight: 12, opacity: 0.28 }).addTo(map);
  const route = L.polyline(cfg.route, { ...line, color: '#c4b5fd', weight: 4, opacity: 0.95 }).addTo(map);
  app.routeBounds = route.getBounds();

  for (const stop of cfg.stops) {
    L.marker([stop.lat, stop.lon], {
      icon: L.divIcon({ className: 'stop-icon', html: '<span></span>', iconSize: [18, 18] }),
      keyboard: false,
      title: stop.name,
    })
      .bindTooltip(escapeHtml(stop.name), {
        permanent: true,
        // Étiquette côté intérieur de la carte, pour ne pas déborder de l'écran.
        direction: 'auto',
        offset: [0, 0],
        className: 'stop-label',
      })
      .addTo(map);
  }

  fitRoute();

  // Tant que l'utilisateur n'a pas touché la carte, on garde tout le trajet
  // cadré (rotation du téléphone, panneau réduit, fenêtre redimensionnée…).
  const stopAutoFit = () => (app.autoFit = false);
  map.getContainer().addEventListener('pointerdown', stopAutoFit);
  map.getContainer().addEventListener('wheel', stopAutoFit, { passive: true });
  map.on('dragstart', () => setFollow(false));
  new ResizeObserver(() => {
    map.invalidateSize();
    if (app.autoFit) fitRoute();
    else if (app.follow && app.busMarker) centerOnBus(false);
  }).observe(document.querySelector('.layout'));

  // Sur mobile, le bouton « Me localiser » reste juste au-dessus du panneau,
  // et le trajet reste cadré quand le contenu du panneau change de hauteur.
  const sheet = $('sheet');
  let refit = 0;
  new ResizeObserver(() => {
    const overlay = getComputedStyle(sheet).position === 'absolute';
    document.querySelector('.map-wrap').style.setProperty('--sheet-h', `${overlay ? sheet.offsetHeight : 0}px`);
    clearTimeout(refit);
    refit = setTimeout(() => app.autoFit && fitRoute(), 200);
  }).observe(sheet);
}

/** Marge pour que le panneau du bas ne cache pas la carte sur mobile. */
function mapPadding() {
  const sheet = $('sheet');
  const overlay = getComputedStyle(sheet).position === 'absolute';
  return {
    paddingTopLeft: [24, 64],
    paddingBottomRight: [24, (overlay ? sheet.offsetHeight : 0) + 24],
  };
}

function fitRoute() {
  app.map.fitBounds(app.routeBounds, mapPadding());
}

function setFollow(on) {
  app.follow = on;
  if (on) app.autoFit = false;
  $('btn-bus').setAttribute('aria-pressed', String(on));
  if (on && app.busMarker) centerOnBus(true);
}

/** Centre de carte qui place `pos` au milieu de la partie visible (au-dessus du panneau). */
function visibleCenter(pos, zoom) {
  const { paddingBottomRight } = mapPadding();
  const target = app.map.project(pos, zoom).add([0, (paddingBottomRight[1] - 24) / 2]);
  return app.map.unproject(target, zoom);
}

function centerOnBus(animate) {
  app.map.panTo(visibleCenter(app.busMarker.getLatLng(), app.map.getZoom()), { animate });
}

function busIcon() {
  return L.divIcon({
    className: 'bus-icon',
    iconSize: [44, 44],
    iconAnchor: [22, 22],
    html: `<div class="bus"><div class="bus-heading"></div><div class="bus-body">${BUS_SVG}</div></div>`,
  });
}

function renderBus(snap, status) {
  const bus = snap.bus;
  if (!bus) {
    if (app.busMarker) {
      app.busMarker.remove();
      app.busMarker = null;
    }
    $('btn-bus').disabled = true;
    setFollow(false);
    return;
  }

  const to = bus.on_route ? [bus.snap_lat, bus.snap_lon] : [bus.lat, bus.lon];
  if (!app.busMarker) {
    app.busMarker = L.marker(to, { icon: busIcon(), zIndexOffset: 1000, keyboard: false, title: 'Navette' }).addTo(app.map);
  } else {
    animateBus(to);
  }
  $('btn-bus').disabled = false;

  const node = app.busMarker.getElement()?.querySelector('.bus');
  if (node) {
    const live = LIVE.has(status);
    node.classList.toggle('live', live);
    node.classList.toggle('dim', !live);
    node.classList.toggle('no-heading', bus.heading == null || status === 'at_stop');
    if (bus.heading != null) node.querySelector('.bus-heading').style.transform = `rotate(${bus.heading}deg)`;
  }
  if (app.follow) centerOnBus(true);
}

function animateBus(to) {
  const marker = app.busMarker;
  const from = marker.getLatLng();
  cancelAnimationFrame(app.anim);
  if (document.hidden || app.map.distance(from, to) > 400) {
    marker.setLatLng(to);
    return;
  }
  const start = performance.now();
  const duration = 1500;
  const step = (t) => {
    const k = Math.min(1, (t - start) / duration);
    marker.setLatLng([from.lat + (to[0] - from.lat) * k, from.lng + (to[1] - from.lng) * k]);
    if (k < 1) app.anim = requestAnimationFrame(step);
  };
  app.anim = requestAnimationFrame(step);
}

// ------------------------------------------------------------------ panneau

function renderStatic(cfg) {
  const schedule = $('schedule');
  for (const line of cfg.event.schedule) schedule.append(el('li', null, line));
  $('info').textContent = cfg.event.info || '';
  if (cfg.event.tickets_url) {
    $('tickets').href = cfg.event.tickets_url;
    $('tickets').hidden = false;
  }
}

/** Le serveur peut ne plus nous parler (réseau mobile) : on vieillit l'état localement. */
function effectiveStatus(snap) {
  if (LIVE.has(snap.status) && snap.bus) {
    const age = (serverNow() - snap.bus.ts) / 1000;
    if (age > app.cfg.stale_after_s) return 'stale';
  }
  return snap.status;
}

function renderStatus(snap, status) {
  const info = STATUS[status] || STATUS.offline;
  const pill = $('status-pill');
  pill.textContent = info.pill;
  pill.className = `pill ${info.cls}`;

  const stopName = (id) => app.stops.get(id)?.name ?? '';
  const next = snap.etas.find((e) => e.stop === snap.next_stop);
  let title = '';
  let sub = '';
  switch (status) {
    case 'moving': {
      title = next ? `En route vers ${stopName(next.stop)}` : 'En route';
      const dep = snap.departure;
      const left = dep?.stop && serverNow() - dep.ts < 30 * 60_000 ? `Partie de ${stopName(dep.stop)} à ${TIME_FMT.format(dep.ts)}. ` : '';
      if (snap.bus && !snap.bus.on_route) sub = 'Déviation : la navette a quitté le trajet habituel.';
      else if (next) sub = `${left}Arrivée estimée dans ${fmtMinutes(next.eta_s)}.`;
      break;
    }
    case 'at_stop':
      // Pas d'horaire : elle part dès qu'elle est pleine, ou quand le chauffeur décide.
      title = `À quai · ${stopName(snap.at_stop)}`;
      sub = 'Montez ! Elle part dès qu’elle est pleine, ou quand le chauffeur décide.';
      break;
    case 'departing':
      title = `Départ de ${stopName(snap.at_stop)}`;
      sub = next ? `La navette s’en va. Arrivée à ${stopName(next.stop)} dans ${fmtMinutes(next.eta_s)}.` : 'La navette s’en va.';
      break;
    case 'stale':
      title = 'Signal perdu';
      sub = 'La navette continue sa boucle, sa position revient dès que le réseau suit.';
      break;
    case 'pause':
      title = 'Pause du chauffeur';
      sub = 'La navette repart dans quelques minutes.';
      break;
    case 'off':
      title = 'Navette hors service';
      sub = 'Consultez les horaires ci-dessous.';
      break;
    default:
      title = 'Navette pas en circulation';
      sub = 'Aucune position récente. Consultez les horaires ci-dessous.';
  }
  if ($('status-title').textContent !== title) $('status-title').textContent = title;
  $('status-sub').textContent = sub;
  renderSeats(snap, status);
}

/** Places libres, si l'équipage tient le compteur à jour. */
function renderSeats(snap, status) {
  const known = snap.passengers != null && LIVE.has(status);
  $('seats').hidden = !known;
  if (!known) return;
  const free = snap.capacity - snap.passengers;
  $('seats').classList.toggle('full', free <= 0);
  $('seats-text').textContent =
    free <= 0 ? `Complet · ${snap.capacity} personnes à bord` : `${free} place${free > 1 ? 's' : ''} libre${free > 1 ? 's' : ''} sur ${snap.capacity}`;
  const bar = $('seats-bar');
  if (bar.childElementCount !== snap.capacity) {
    bar.replaceChildren(...Array.from({ length: snap.capacity }, () => el('i')));
  }
  [...bar.children].forEach((seat, i) => seat.classList.toggle('taken', i < snap.passengers));
}

function renderEtas(snap, status) {
  const list = $('etas');
  list.replaceChildren();
  const live = LIVE.has(status);
  // À quai, le départ n'est pas fixé : les temps comptent à partir du départ.
  const waiting = status === 'at_stop';
  const etas = live && snap.etas.length ? snap.etas : app.cfg.stops.map((s) => ({ stop: s.id, unknown: true }));
  const near = app.locate.me && nearestStop(app.locate.me);

  for (const eta of etas) {
    const stop = app.stops.get(eta.stop);
    if (!stop) continue;
    const here = !eta.unknown && eta.eta_s === 0;
    const li = el('li', 'eta');
    li.classList.toggle('next', eta.stop === snap.next_stop || here);
    li.classList.toggle('here', here);
    li.classList.toggle('unknown', !!eta.unknown);

    const main = el('div', 'eta-main');
    main.append(el('div', 'eta-name', stop.name));
    if (stop.detail) main.append(el('div', 'eta-detail', stop.detail));
    if (near?.stop === stop) main.append(el('div', 'eta-near', nearText(near)));

    const time = el('div', 'eta-time');
    if (eta.unknown) {
      time.append(el('strong', null, '—'));
    } else if (here) {
      time.append(el('strong', null, 'À quai'));
    } else if (waiting) {
      time.append(el('strong', null, fmtMinutes(eta.eta_s)), el('small', null, 'après le départ'));
    } else {
      time.append(el('strong', null, fmtMinutes(eta.eta_s)), el('small', null, fmtDistance(eta.distance_m)));
    }

    li.append(el('span', 'eta-dot'), main, time);
    list.append(li);
  }
}

function renderBanner(message) {
  const banner = $('banner');
  const wasHidden = banner.hidden;
  banner.hidden = !message;
  if (message) {
    banner.textContent = message.text;
    banner.dataset.level = message.level;
  }
  if (wasHidden !== banner.hidden) app.map.invalidateSize();
}

// ------------------------------------------------------------------ trains

/** Départs en gare, déjà filtrés et mis en forme par le serveur ; ici, le compte à rebours. */
function renderTrains() {
  const board = app.trains;
  if (!board) return;
  const now = serverNow();
  const upcoming = board.departures
    .map((d) => ({ ...d, expected: d.time + d.delay_min * 60_000 }))
    .filter((d) => d.expected > now - 30_000)
    .slice(0, board.limit)
    .map((d) => ({ ...d, countdown: fmtCountdown((d.expected - now) / 1000) }));
  const stale = board.updated != null && now - board.updated > 5 * 60_000;

  // Le compte à rebours est recalculé chaque seconde : on ne touche au DOM
  // que si l'affichage change.
  const key = JSON.stringify([upcoming.map((d) => [d.time, d.delay_min, d.platform, d.countdown]), stale, board.updated]);
  if (key === app.trainsKey) return;
  app.trainsKey = key;

  const list = $('trains');
  list.replaceChildren();
  for (const d of upcoming) {
    const li = el('li', 'train');
    const time = el('div', 'train-time');
    time.append(el('strong', null, TIME_FMT.format(d.time)));
    if (d.delay_min > 0) time.append(el('small', 'late', `+${d.delay_min} min`));

    const main = el('div', 'train-main');
    main.append(el('div', 'train-to', d.to));
    const meta = el('div', 'train-meta');
    const line = el('span', 'train-line', d.line);
    line.dataset.cat = d.category;
    meta.append(line);
    if (d.platform) meta.append(el('span', d.platform_changed ? 'changed' : null, `Voie ${d.platform}`));
    main.append(meta);

    li.append(time, main, el('div', 'train-in', d.countdown));
    list.append(li);
  }

  let note = '';
  if (board.updated == null) note = 'Horaires CFF momentanément indisponibles.';
  else if (stale) note = `Horaires de ${TIME_FMT.format(board.updated)} : les retards ne sont peut-être plus à jour.`;
  else if (!upcoming.length) note = 'Aucun départ annoncé pour le moment.';
  $('trains-note').textContent = note;
}

async function refreshTrains() {
  try {
    app.trains = await fetchJson('/api/trains');
    renderTrains();
  } catch {
    // On garde les derniers départs connus.
  }
}

// ------------------------------------------------------------------ ma position

// Calculé ici, dans le navigateur : la position du visiteur n'est envoyée nulle part.

function nearestStop(me) {
  let best = null;
  for (const stop of app.cfg.stops) {
    const distance = L.latLng(me.lat, me.lon).distanceTo([stop.lat, stop.lon]);
    if (!best || distance < best.distance) best = { stop, distance };
  }
  return best;
}

function nearText({ stop, distance }) {
  if (distance <= Math.max(stop.radius_m, 40)) return 'Tu es à cet arrêt';
  if (distance > 3000) return `Arrêt le plus proche · ${fmtDistance(distance)}`;
  const walk = Math.max(1, Math.round((distance * WALK_DETOUR) / WALK_SPEED / 60));
  const approx = app.locate.me.acc > 200 ? ' (position approximative)' : '';
  return `Le plus proche · ${fmtDistance(distance)}, ≈ ${walk} min à pied${approx}`;
}

function toast(text) {
  const t = $('toast');
  t.textContent = text;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.hidden = true), 6000);
}

function setLocateButton(state) {
  const btn = $('btn-locate');
  btn.dataset.state = state;
  btn.setAttribute('aria-pressed', String(state !== 'off'));
  $('btn-locate-text').textContent = state === 'pending' ? 'Localisation…' : 'Me localiser';
}

function startLocate() {
  if (!('geolocation' in navigator) || !window.isSecureContext) {
    toast("La localisation n'est pas disponible sur cette page.");
    return;
  }
  app.locate.on = true;
  app.locate.framed = false;
  setLocateButton('pending');
  watchLocation();
}

function stopLocate() {
  unwatchLocation();
  app.locate.on = false;
  app.locate.me = null;
  if (app.locate.layers) {
    for (const layer of Object.values(app.locate.layers)) layer.remove();
    app.locate.layers = null;
  }
  setLocateButton('off');
  if (app.snap) renderEtas(app.snap, tick.lastStatus);
}

function watchLocation() {
  if (app.locate.watch != null) return;
  app.locate.watch = navigator.geolocation.watchPosition(onPosition, onPositionError, {
    enableHighAccuracy: true,
    maximumAge: 10_000,
    timeout: 20_000,
  });
}

function unwatchLocation() {
  if (app.locate.watch == null) return;
  navigator.geolocation.clearWatch(app.locate.watch);
  app.locate.watch = null;
}

function onPosition(pos) {
  if (!app.locate.on) return;
  const { latitude, longitude, accuracy } = pos.coords;
  app.locate.me = { lat: latitude, lon: longitude, acc: accuracy, at: Date.now() };
  setLocateButton('on');
  drawMe();
  if (app.snap) renderEtas(app.snap, tick.lastStatus);
}

function onPositionError(err) {
  // Un retard ponctuel du GPS : on garde la dernière position connue.
  if (err.code === err.TIMEOUT && app.locate.me) return;
  stopLocate();
  toast(GEO_ERRORS[err.code] || GEO_ERRORS[2]);
}

function drawMe() {
  const { me } = app.locate;
  const near = nearestStop(me);
  const pos = [me.lat, me.lon];
  const stopPos = [near.stop.lat, near.stop.lon];
  const blue = '#38bdf8';
  let layers = app.locate.layers;
  if (!layers) {
    layers = app.locate.layers = {
      line: L.polyline([pos, stopPos], { color: blue, weight: 3, opacity: 0.9, dashArray: '1 8', lineCap: 'round', interactive: false }),
      circle: L.circle(pos, { radius: me.acc, color: blue, weight: 1, opacity: 0.4, fillOpacity: 0.08, interactive: false }),
      marker: L.marker(pos, {
        icon: L.divIcon({ className: 'me-icon', html: '<span></span>', iconSize: [20, 20] }),
        zIndexOffset: 500,
        keyboard: false,
        interactive: false,
      }),
    };
    for (const layer of Object.values(layers)) layer.addTo(app.map);
  } else {
    layers.line.setLatLngs([pos, stopPos]);
    layers.circle.setLatLng(pos).setRadius(me.acc);
    layers.marker.setLatLng(pos);
  }

  // Une seule fois : on cadre le visiteur et son arrêt, sans lui reprendre la carte ensuite.
  if (app.locate.framed) return;
  app.locate.framed = true;
  if (near.distance > 15_000) {
    toast(`Tu es à ${fmtDistance(near.distance)} de l'arrêt le plus proche.`);
    return;
  }
  setFollow(false);
  app.autoFit = false;
  app.map.fitBounds(L.latLngBounds([pos, stopPos]).pad(0.25), { ...mapPadding(), maxZoom: 17 });
}

// ------------------------------------------------------------------ faire signe

// Pour qui est sur le trajet, loin d'un arrêt : le serveur vérifie la position,
// la recale sur la route et ne garde que ce point, jusqu'au passage de la navette.

function deviceId() {
  const fresh = () => crypto.randomUUID?.() ?? `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  try {
    let id = localStorage.getItem(DEVICE_KEY);
    if (!id) {
      id = fresh();
      localStorage.setItem(DEVICE_KEY, id);
    }
    return id;
  } catch {
    deviceId.fallback ??= fresh();
    return deviceId.fallback;
  }
}

function savePickup(p) {
  app.pickup = p;
  try {
    if (p) localStorage.setItem(PICKUP_KEY, JSON.stringify(p));
    else localStorage.removeItem(PICKUP_KEY);
  } catch {
    // Sans stockage, la demande est simplement oubliée au rechargement.
  }
  drawPickupMarker();
}

function loadPickup() {
  try {
    const p = JSON.parse(localStorage.getItem(PICKUP_KEY));
    if (p && Date.now() - p.created < 60 * 60_000) app.pickup = p;
  } catch {
    // Rien à reprendre.
  }
}

function drawPickupMarker() {
  app.pickupMarker?.remove();
  app.pickupMarker = null;
  const p = app.pickup;
  if (!p || p.closed) return;
  app.pickupMarker = L.marker([p.lat, p.lon], {
    icon: L.divIcon({ className: 'pickup-icon', html: '<span></span>', iconSize: [22, 22] }),
    zIndexOffset: 600,
    keyboard: false,
  })
    .bindTooltip('Tu attends ici', { permanent: true, direction: 'top', offset: [0, -12], className: 'stop-label' })
    .addTo(app.map);
}

function freeSeats(snap) {
  return snap?.passengers != null ? snap.capacity - snap.passengers : null;
}

function renderPickup(snap, status) {
  if (!app.cfg.pickups) return;
  const p = app.pickup;
  const running = LIVE.has(status) || status === 'stale';
  $('pickup-idle').hidden = !!p;
  $('pickup-active').hidden = !p;
  $('btn-pickup').disabled = !running;
  $('pickup-unavailable').textContent = running ? '' : 'Possible uniquement quand la navette circule.';
  if (!p) return;

  const who = `${p.count} pers. vers ${app.stops.get(p.dest)?.name ?? ''}`;
  const active = snap.pickups.find((x) => x.id === p.id);
  const closed = snap.pickups_closed.find((x) => x.id === p.id);

  if (active) {
    let sub;
    if (status === 'at_stop') {
      sub = `La navette attend à quai. Elle passera environ ${fmtMinutes(active.eta_s ?? 0)} après son départ.`;
    } else if (active.eta_s != null) {
      sub = `Elle passe dans environ ${fmtMinutes(active.eta_s)}. Reste bien visible au bord de la route.`;
    } else {
      sub = 'Position de la navette inconnue pour le moment.';
    }
    const free = freeSeats(snap);
    if (free != null && free < p.count) {
      sub += free <= 0 ? ' Attention : elle est pleine pour l’instant.' : ` Attention : il ne reste que ${free} place${free > 1 ? 's' : ''}.`;
    }
    $('pickup-title').textContent = `Le chauffeur sait que tu attends · ${who}`;
    $('pickup-sub').textContent = sub;
    $('btn-pickup-cancel').textContent = 'Annuler ma demande';
    return;
  }
  // Juste envoyée : l'état diffusé ne la contient peut-être pas encore.
  if (!closed && !p.closed && snap.server_time - p.since < 10_000) return;
  if (!p.closed) savePickup({ ...p, closed: true });
  $('pickup-title').textContent = who;
  $('pickup-sub').textContent = PICKUP_OUTCOMES[closed?.outcome] ?? 'Cette demande n’est plus active.';
  $('btn-pickup-cancel').textContent = 'OK';
}

function setPickupDest(id) {
  app.pickupDest = id;
  for (const b of $('pickup-dest').children) b.setAttribute('aria-pressed', String(b.dataset.stop === id));
}

function setPickupCount(n) {
  const max = app.cfg.pickups.max_group;
  app.pickupCount = Math.max(1, Math.min(max, n));
  $('pickup-count').textContent = app.pickupCount;
  $('pickup-minus').disabled = app.pickupCount <= 1;
  $('pickup-plus').disabled = app.pickupCount >= max;
  const free = freeSeats(app.snap);
  let note = '';
  if (free != null && free <= 0) note = 'La navette est pleine pour l’instant : elle ne pourra peut-être pas s’arrêter.';
  else if (free != null && free < app.pickupCount) note = `Il ne reste que ${free} place${free > 1 ? 's' : ''} à bord.`;
  else if (free != null) note = `${free} place${free > 1 ? 's' : ''} libre${free > 1 ? 's' : ''} à bord en ce moment.`;
  $('pickup-seats').textContent = note;
}

function openPickupDialog() {
  const box = $('pickup-dest');
  if (!box.childElementCount) {
    for (const stop of app.cfg.stops) {
      const b = el('button', 'choice', `Vers ${stop.name}`);
      b.type = 'button';
      b.dataset.stop = stop.id;
      box.append(b);
    }
  }
  setPickupDest(null);
  setPickupCount(app.pickupCount);
  $('pickup-error').textContent = '';
  $('pickup-dialog').showModal();
}

/** Position récente de « Me localiser » si elle est assez précise, sinon une nouvelle. */
function currentPosition() {
  const me = app.locate.me;
  if (me && me.acc <= 100 && Date.now() - me.at < 30_000) return Promise.resolve(me);
  return new Promise((resolve, reject) => {
    navigator.geolocation.getCurrentPosition(
      (pos) => resolve({ lat: pos.coords.latitude, lon: pos.coords.longitude, acc: pos.coords.accuracy }),
      reject,
      { enableHighAccuracy: true, timeout: 20_000, maximumAge: 10_000 },
    );
  });
}

async function sendPickup(event) {
  event.preventDefault();
  const error = $('pickup-error');
  if (!app.pickupDest) {
    error.textContent = 'Choisis ta direction.';
    return;
  }
  if (!('geolocation' in navigator) || !window.isSecureContext) {
    error.textContent = "La localisation n'est pas disponible sur cette page.";
    return;
  }
  const btn = $('pickup-send');
  btn.disabled = true;
  btn.textContent = 'Localisation…';
  error.textContent = '';
  try {
    const me = await currentPosition();
    btn.textContent = 'Envoi…';
    const res = await fetch('/api/pickups', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        lat: me.lat,
        lon: me.lon,
        accuracy: me.acc,
        count: app.pickupCount,
        dest: app.pickupDest,
        device: deviceId(),
      }),
    });
    if (res.status !== 201) {
      const body = await res.json().catch(() => null);
      error.textContent = body?.error ?? 'La demande n’a pas pu être envoyée. Réessaie.';
      return;
    }
    const c = await res.json();
    savePickup({ id: c.id, token: c.token, lat: c.lat, lon: c.lon, count: app.pickupCount, dest: app.pickupDest, created: Date.now(), since: serverNow() });
    $('pickup-dialog').close();
    setFollow(false);
    app.autoFit = false;
    const zoom = Math.max(app.map.getZoom(), 16);
    app.map.setView(visibleCenter(L.latLng(c.lat, c.lon), zoom), zoom);
    refresh();
  } catch (err) {
    error.textContent = err?.code ? GEO_ERRORS[err.code] || GEO_ERRORS[2] : 'Pas de réseau : réessaie dans un instant.';
  } finally {
    btn.disabled = false;
    btn.textContent = 'Envoyer';
  }
}

async function cancelPickup() {
  const p = app.pickup;
  if (p && !p.closed) {
    await fetch('/api/pickups/cancel', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id: p.id, token: p.token }),
    }).catch(() => {});
  }
  savePickup(null);
  if (app.snap) renderPickup(app.snap, tick.lastStatus);
}

function setupPickups() {
  if (!app.cfg.pickups) return;
  $('pickup-block').hidden = false;
  loadPickup();
  drawPickupMarker();
  $('btn-pickup').addEventListener('click', openPickupDialog);
  $('btn-pickup-cancel').addEventListener('click', cancelPickup);
  $('pickup-close').addEventListener('click', () => $('pickup-dialog').close());
  $('pickup-minus').addEventListener('click', () => setPickupCount(app.pickupCount - 1));
  $('pickup-plus').addEventListener('click', () => setPickupCount(app.pickupCount + 1));
  $('pickup-dest').addEventListener('click', (e) => {
    const id = e.target.closest('button')?.dataset.stop;
    if (id) setPickupDest(id);
  });
  $('pickup-form').addEventListener('submit', sendPickup);
}

// ------------------------------------------------------------------ rendu

function render(snap) {
  app.snap = snap;
  const status = effectiveStatus(snap);
  tick.lastStatus = status;
  renderBanner(snap.message);
  renderStatus(snap, status);
  renderEtas(snap, status);
  renderBus(snap, status);
  renderPickup(snap, status);
  tick();
}

/** Chaque seconde : âge de la position, et passage en « signal perdu » si besoin. */
function tick() {
  renderTrains();
  const snap = app.snap;
  if (!snap) return;
  const status = effectiveStatus(snap);
  if (status !== tick.lastStatus) {
    tick.lastStatus = status;
    renderStatus(snap, status);
    renderEtas(snap, status);
    renderBus(snap, status);
    renderPickup(snap, status);
  }
  const updated = $('updated');
  if (snap.bus) {
    const age = Math.max(0, Math.round((serverNow() - snap.bus.ts) / 1000));
    updated.textContent = `Position mise à jour il y a ${fmtAgo(age)}`;
  } else {
    updated.textContent = '';
  }
}

// ------------------------------------------------------------------ temps réel

function setConn(state) {
  $('conn').dataset.state = state;
  $('conn-text').textContent = state === 'live' ? 'En direct' : state === 'retry' ? 'Reconnexion…' : 'Connexion…';
}

/** Récupère un état frais (et l'heure du serveur, pour calculer les âges). */
async function refresh() {
  try {
    const snap = await fetchJson('/api/state');
    app.clockOffset = snap.server_time - Date.now();
    render(snap);
    if (!app.es) setConn('retry');
  } catch {
    setConn('retry');
  }
}

function connect() {
  if (app.es || !('EventSource' in window)) return;
  const es = new EventSource('/api/stream');
  app.es = es;
  es.onopen = () => setConn('live');
  es.onmessage = (e) => {
    setConn('live');
    render(JSON.parse(e.data));
  };
  es.onerror = () => {
    setConn('retry');
    // Le navigateur se reconnecte seul, sauf si le serveur a refusé la connexion.
    if (es.readyState === EventSource.CLOSED) {
      app.es = null;
      setTimeout(connect, 5000);
    }
  };
}

function disconnect() {
  app.es?.close();
  app.es = null;
}

// Téléphone en poche = pas de connexion ouverte ni de GPS allumé pour rien.
document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    disconnect();
    unwatchLocation();
  } else {
    refresh();
    connect();
    if (app.cfg?.trains) refreshTrains();
    if (app.locate.on) watchLocation();
  }
});

async function init() {
  app.cfg = await fetchJsonRetry('/api/config');
  for (const stop of app.cfg.stops) app.stops.set(stop.id, stop);
  renderStatic(app.cfg);
  setupMap(app.cfg);
  setupPickups();

  $('btn-bus').addEventListener('click', () => setFollow(!app.follow));
  $('btn-locate').addEventListener('click', () => (app.locate.on ? stopLocate() : startLocate()));
  $('btn-route').addEventListener('click', () => {
    setFollow(false);
    app.autoFit = true;
    fitRoute();
  });
  $('handle').addEventListener('click', () => {
    const collapsed = $('sheet').classList.toggle('collapsed');
    $('handle').setAttribute('aria-expanded', String(!collapsed));
    setTimeout(() => {
      if (app.autoFit) fitRoute();
      else if (app.follow && app.busMarker) centerOnBus(true);
    }, 260);
  });

  if (app.cfg.trains) {
    $('trains-title').textContent = `Prochains trains · ${app.cfg.trains.station}`;
    $('trains-block').hidden = false;
    refreshTrains();
    setInterval(() => document.hidden || refreshTrains(), TRAINS_REFRESH_MS);
  }

  await refresh();
  connect();
  setInterval(tick, 1000);
  setInterval(() => document.hidden || refresh(), REFRESH_MS);
}

init();

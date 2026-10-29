'use strict';

// Commandes de l'équipage, partagées par la page chauffeur et la page orga :
// personnes à bord, annonce du départ, signes des gens qui attendent sur le
// trajet, mode de service et message aux festivaliers. Toute la logique
// (arrivée, remise à zéro, passage) est côté serveur.

(() => {
  const POLL_MS = 3000;
  /** Après un appui, le compteur local fait foi le temps que le serveur suive. */
  const LOCAL_HOLD_MS = 4000;
  /** Un nouveau signe reste en surbrillance… */
  const NEW_MS = 20_000;

  const TIME_FMT = new Intl.DateTimeFormat('fr-CH', { hour: '2-digit', minute: '2-digit', timeZone: 'Europe/Zurich' });

  const PRESETS = [
    'Navette retardée d\'environ 10 minutes.',
    'Navette pleine, elle revient dans une quinzaine de minutes.',
    'Dernier départ avant la pause : la navette reprend à 04:20.',
    'Pause jusqu\'à 04:20 : la navette reprend ensuite jusqu\'à 06:30.',
    'Dernier départ de l\'Usine de Chandoline dans 15 minutes.',
    'Trafic perturbé, merci de votre patience.',
  ];

  /** Tant qu'une demande d'aide n'a pas de réponse, l'alarme se répète. */
  const ALARM_MS = 6000;

  // Petite carte de l'équipage : même fond swisstopo que la page publique.
  const TILE_URL = 'https://wmts.geo.admin.ch/1.0.0/ch.swisstopo.pixelkarte-grau/default/current/3857/{z}/{x}/{y}.jpeg';
  const STOP_COLORS = ['#2f66d8', '#f2c300', '#e2321f', '#f2f1ec'];

  const TEMPLATE = `
    <section class="crew-alerts" hidden>
      <h2>Demandes d'aide</h2>
      <ol></ol>
      <p class="crew-note">Position exacte, visible seulement par l'équipage. Touche une alerte pour la voir sur la carte. Si c'est grave, appelle le 117 (police) ou le 144 (ambulance).</p>
    </section>
    <section class="crew-map-wrap">
      <div class="crew-map" role="region" aria-label="Carte : navette, signes et demandes d'aide"></div>
      <p class="crew-legend"><i class="lg-bus"></i>Navette <i class="lg-pickup"></i>Signe <i class="lg-alert"></i>Aide</p>
    </section>
    <section class="crew-seats">
      <div class="crew-label">Personnes à bord</div>
      <div class="crew-counter">
        <button type="button" class="crew-step" data-delta="-1" aria-label="Une personne de moins"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14"/></svg></button>
        <output class="crew-count" aria-live="polite">—</output>
        <button type="button" class="crew-step" data-delta="1" aria-label="Une personne de plus"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14M12 5v14"/></svg></button>
      </div>
      <div class="crew-strip" aria-hidden="true"></div>
      <div class="crew-free"></div>
      <div class="crew-quick">
        <button type="button" data-set="empty">Vide</button>
        <button type="button" data-set="full">Plein</button>
      </div>
    </section>
    <button type="button" class="crew-depart">On part</button>
    <p class="crew-note crew-depart-note"></p>
    <section class="crew-pickups">
      <h2>Signes sur le trajet</h2>
      <ol></ol>
      <p class="crew-note crew-empty">Personne n'attend sur le trajet.</p>
    </section>
    <p class="crew-note">À manipuler à l'arrêt, jamais en roulant. Le compteur repart de zéro à chaque arrêt.</p>
    <section class="crew-service">
      <h2>Service</h2>
      <div class="crew-modes">
        <button type="button" data-mode="auto" aria-pressed="false">Auto</button>
        <button type="button" data-mode="pause" aria-pressed="false">Pause</button>
        <button type="button" data-mode="off" aria-pressed="false">Hors service</button>
      </div>
      <p class="crew-note">Auto : le statut suit les positions du téléphone. Pause : position visible, sans temps d'attente. Hors service : navette masquée.</p>
    </section>
    <section class="crew-message">
      <h2>Message aux festivaliers</h2>
      <textarea class="crew-msg" maxlength="280" aria-label="Message aux festivaliers" placeholder="Ex. : Navette retardée d'environ 10 minutes."></textarea>
      <div class="crew-presets">${PRESETS.map((p) => `<button type="button" data-preset>${p}</button>`).join('')}</div>
      <select class="crew-level" aria-label="Niveau du message">
        <option value="info">Info (bandeau bleu)</option>
        <option value="warn">Attention (bandeau jaune)</option>
        <option value="alert">Urgent (bandeau rouge)</option>
      </select>
      <div class="crew-actions">
        <button type="button" class="crew-publish">Publier</button>
        <button type="button" class="crew-clear">Effacer</button>
      </div>
      <p class="crew-note crew-current" aria-live="polite"></p>
    </section>`;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function fmtDistance(m) {
    return m < 1000 ? `${Math.round(m / 10) * 10} m` : `${(m / 1000).toFixed(1).replace('.', ',')} km`;
  }

  function fmtAhead(p) {
    if (p.distance_m == null) return 'navette pas localisée';
    return `dans ${fmtDistance(p.distance_m)} · ~${Math.max(1, Math.round(p.eta_s / 60))} min`;
  }

  function fmtAgo(ms) {
    const m = Math.floor(Math.max(0, ms) / 60_000);
    return m < 1 ? "à l'instant" : `il y a ${m} min`;
  }

  /**
   * Carte : trajet, arrêts, navette, signes (losanges jaunes) et demandes
   * d'aide (rouge, avec le cercle de précision). Elle cadre tout le trajet tant
   * qu'on n'y touche pas, et se recadre d'office sur une nouvelle demande d'aide.
   */
  function crewMap(container) {
    if (!window.L) {
      container.parentElement.hidden = true;
      return null;
    }
    const map = L.map(container, { attributionControl: false, zoomControl: false });
    L.control.zoom({ position: 'topright' }).addTo(map);
    L.control.attribution({ prefix: false, position: 'bottomleft' }).addTo(map);
    L.tileLayer(TILE_URL, { minZoom: 11, maxNativeZoom: 18, maxZoom: 19, attribution: '© swisstopo' }).addTo(map);
    // `refit` : le dernier cadrage automatique, rejoué si la carte change de
    // taille, tant que personne n'y a touché.
    const m = { map, routeBounds: null, bus: null, pickups: new Map(), alerts: new Map(), refit: null, alertIds: '' };
    const touched = () => (m.refit = null);
    container.addEventListener('pointerdown', touched);
    container.addEventListener('wheel', touched, { passive: true });
    new ResizeObserver(() => {
      map.invalidateSize();
      m.refit?.();
    }).observe(container);

    m.base = L.layerGroup().addTo(map);
    loadRoute(m);
    return m;
  }

  /** Trajet et arrêts, rechargés quand l'orga change le tracé. */
  function loadRoute(m, version) {
    m.routeVersion = version;
    fetch('/api/config', { cache: 'no-store' })
      .then((r) => r.json())
      .then((cfg) => {
        m.routeVersion = cfg.route_version;
        m.base.clearLayers();
        const line = L.polyline(cfg.route, { color: '#f2f1ec', weight: 3, opacity: 0.9, interactive: false }).addTo(m.base);
        m.routeBounds = line.getBounds();
        cfg.stops.forEach((stop, i) => {
          L.marker([stop.lat, stop.lon], {
            icon: L.divIcon({ className: 'crew-map-stop', html: `<span style="background:${STOP_COLORS[i % STOP_COLORS.length]}"></span>`, iconSize: [14, 14] }),
            title: stop.name,
            keyboard: false,
          }).addTo(m.base);
        });
        // Un cadrage sur une demande d'aide reste prioritaire.
        if (!m.alertIds) fitAll(m);
      })
      .catch(() => (m.routeVersion = null));
  }

  /** Cadre tout : trajet, navette, signes et alertes. */
  function fitAll(m) {
    if (!m.routeBounds) return;
    m.refit = () => {
      const bounds = L.latLngBounds(m.routeBounds.getSouthWest(), m.routeBounds.getNorthEast());
      if (m.bus) bounds.extend(m.bus.getLatLng());
      for (const { marker } of [...m.pickups.values(), ...m.alerts.values()]) bounds.extend(marker.getLatLng());
      m.map.fitBounds(bounds, { padding: [20, 20], maxZoom: 17 });
    };
    m.refit();
  }

  /** Les marqueurs sont mis à jour sur place, pour ne pas faire clignoter les étiquettes. */
  function syncMarkers(m, store, items, make, update) {
    const ids = new Set(items.map((x) => x.id));
    for (const [id, entry] of store) {
      if (!ids.has(id)) {
        entry.marker.remove();
        entry.circle?.remove();
        store.delete(id);
      }
    }
    for (const item of items) {
      let entry = store.get(item.id);
      if (!entry) {
        entry = make(item);
        store.set(item.id, entry);
      }
      update(entry, item);
    }
  }

  function renderMap(m, data) {
    if (!m) return;
    const version = data.snapshot.route_version;
    if (version && m.routeVersion !== undefined && m.routeVersion !== version) loadRoute(m, version);
    const bus = data.snapshot.bus;
    if (bus) {
      const pos = bus.on_route ? [bus.snap_lat, bus.snap_lon] : [bus.lat, bus.lon];
      if (!m.bus) {
        m.bus = L.marker(pos, { icon: L.divIcon({ className: 'crew-map-bus', html: '<span></span>', iconSize: [18, 18] }), zIndexOffset: 500, keyboard: false, title: 'Navette' }).addTo(m.map);
      } else {
        m.bus.setLatLng(pos);
      }
    } else if (m.bus) {
      m.bus.remove();
      m.bus = null;
    }

    syncMarkers(
      m,
      m.pickups,
      data.pickups,
      (p) => ({
        marker: L.marker([p.lat, p.lon], { icon: L.divIcon({ className: 'crew-map-pickup', html: '<span></span>', iconSize: [18, 18] }), keyboard: false })
          .bindTooltip('', { permanent: true, direction: 'top', offset: [0, -10], className: 'crew-map-label' })
          .addTo(m.map),
      }),
      (entry, p) => entry.marker.setTooltipContent(`${p.count} pers. → ${p.dest_name}`),
    );

    const now = data.snapshot.server_time;
    syncMarkers(
      m,
      m.alerts,
      data.alerts ?? [],
      (a) => ({
        circle: L.circle([a.lat, a.lon], { radius: a.accuracy ?? 20, color: '#e2321f', weight: 1, fillOpacity: 0.12, interactive: false }).addTo(m.map),
        marker: L.marker([a.lat, a.lon], { icon: L.divIcon({ className: 'crew-map-alert', html: '<span>!</span>', iconSize: [26, 26] }), zIndexOffset: 1000, keyboard: false })
          .bindTooltip('', { permanent: true, direction: 'top', offset: [0, -14], className: 'crew-map-label alert' })
          .addTo(m.map),
      }),
      (entry, a) => {
        entry.marker.getElement()?.classList.toggle('seen', a.seen != null);
        entry.marker.setTooltipContent(`Aide · ${fmtAgo(now - a.created)}`);
      },
    );

    // Nouvelle demande d'aide : on la montre, même si la carte a été déplacée.
    const alertIds = (data.alerts ?? []).map((a) => a.id).join();
    const fresh = (data.alerts ?? []).some((a) => !m.alertIds.split(',').includes(a.id));
    m.alertIds = alertIds;
    if (fresh) {
      const points = (data.alerts ?? []).map((a) => [a.lat, a.lon]);
      if (m.bus) points.push(m.bus.getLatLng());
      m.refit = () => m.map.fitBounds(L.latLngBounds(points), { padding: [50, 50], maxZoom: 16 });
      m.refit();
    } else if (m.refit && !alertIds) {
      fitAll(m);
    }
  }

  function focusMap(m, lat, lon) {
    if (!m) return;
    m.refit = null;
    m.map.setView([lat, lon], 17);
    m.map.getContainer().scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }

  /**
   * Confirmation sans fenêtre du navigateur (certains navigateurs intégrés la
   * bloquent et répondent « non » d'office) : un premier appui arme le bouton,
   * un second dans les 4 s valide.
   */
  function armed(button, label) {
    if (button.dataset.armed) {
      clearTimeout(Number(button.dataset.armed));
      delete button.dataset.armed;
      button.textContent = button.dataset.label;
      return true;
    }
    button.dataset.label = button.textContent;
    button.textContent = label;
    button.dataset.armed = String(
      setTimeout(() => {
        delete button.dataset.armed;
        button.textContent = button.dataset.label;
      }, 4000),
    );
    return false;
  }

  /** Trois bips et une vibration : une demande d'aide attend une réponse. */
  function alarm() {
    navigator.vibrate?.([400, 150, 400, 150, 400]);
    beep();
    setTimeout(beep, 650);
    setTimeout(beep, 1300);
  }

  /** Bip court pour un nouveau signe (après un premier appui sur la page). */
  function beep() {
    try {
      beep.ctx ??= new AudioContext();
      const ctx = beep.ctx;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.frequency.value = 880;
      gain.gain.setValueAtTime(0.3, ctx.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.5);
      osc.connect(gain).connect(ctx.destination);
      osc.start();
      osc.stop(ctx.currentTime + 0.5);
    } catch {
      // Pas de son : la vibration et la surbrillance suffisent.
    }
  }

  /**
   * Monte les commandes dans `root`.
   * `getToken()` : jeton chauffeur ou orga ; `onState(data)` : chaque état reçu
   * (`null` si le serveur ne répond pas) ; `onUnauthorized()` : jeton refusé.
   */
  window.mountCrew = (root, { getToken, onState, onUnauthorized }) => {
    root.classList.add('crew');
    root.innerHTML = TEMPLATE;
    const q = (sel) => root.querySelector(sel);
    const mapState = crewMap(q('.crew-map'));
    let capacity = 16;
    let count = null;
    let holdUntil = 0;
    let sendTimer = 0;
    let names = new Map();
    const seen = new Map(); // id → première apparition
    const rows = new Map(); // id → ligne affichée
    const alertRows = new Map(); // id → carte d'alerte affichée
    let alarmTimer = 0;
    let firstLoad = true;
    let noteUntil = 0;

    async function api(path, body) {
      const res = await fetch(path, {
        method: body === undefined ? 'GET' : 'POST',
        headers: { Authorization: `Bearer ${getToken()}`, ...(body !== undefined && { 'Content-Type': 'application/json' }) },
        body: body === undefined ? undefined : JSON.stringify(body),
        cache: 'no-store',
      });
      if (res.status === 401) {
        onUnauthorized?.();
        throw new Error('jeton refusé');
      }
      if (!res.ok) throw new Error(String(res.status));
      return res.status === 204 ? null : res.json();
    }

    function drawCount() {
      q('.crew-count').textContent = count ?? '—';
      q('.crew-count').dataset.capacity = `/ ${capacity}`;
      const free = count == null ? null : capacity - count;
      q('.crew-free').textContent = free == null ? 'Compteur pas encore utilisé' : free <= 0 ? 'Complet' : `${free} place${free > 1 ? 's' : ''} libre${free > 1 ? 's' : ''}`;
      // Une case par siège, comme sur la page publique.
      const strip = q('.crew-strip');
      if (strip.childElementCount !== capacity) strip.replaceChildren(...Array.from({ length: capacity }, () => el('i')));
      [...strip.children].forEach((cell, i) => cell.classList.toggle('taken', count != null && i < count));
      strip.classList.toggle('full', free != null && free <= 0);
    }

    function setCount(n) {
      count = Math.max(0, Math.min(capacity, n));
      holdUntil = Date.now() + LOCAL_HOLD_MS;
      drawCount();
      clearTimeout(sendTimer);
      sendTimer = setTimeout(() => api('/api/crew/passengers', { passengers: count }).catch(() => {}), 300);
    }

    /** Demandes d'aide, en tête : mises à jour sur place, comme les signes. */
    function renderAlerts(data) {
      const alerts = data.alerts ?? [];
      const list = q('.crew-alerts ol');
      const ids = new Set(alerts.map((a) => a.id));
      for (const [id, li] of alertRows) {
        if (!ids.has(id)) {
          li.remove();
          alertRows.delete(id);
        }
      }
      for (const a of alerts) {
        let li = alertRows.get(a.id);
        if (!li) {
          li = el('li', 'crew-alert');
          const main = el('div', 'crew-alert-main');
          main.append(el('strong'), el('span', 'crew-alert-note'), el('span', 'crew-alert-meta'));
          const actions = el('div', 'crew-alert-actions');
          const seen = el('button', 'crew-alert-go', "J'y vais");
          seen.type = 'button';
          seen.dataset.alert = a.id;
          seen.dataset.alertAction = 'seen';
          const route = el('a', 'crew-alert-map', 'Itinéraire');
          route.target = '_blank';
          route.rel = 'noopener';
          const close = el('button', null, 'Clore');
          close.type = 'button';
          close.dataset.alert = a.id;
          close.dataset.alertAction = 'closed';
          actions.append(seen, route, close);
          li.append(main, actions);
          alertRows.set(a.id, li);
        }
        li.classList.toggle('seen', a.seen != null);
        Object.assign(li.querySelector('.crew-alert-main').dataset, { lat: a.lat, lon: a.lon });
        const ago = fmtAgo(data.snapshot.server_time - a.created);
        li.querySelector('strong').textContent = a.seen != null ? `Quelqu'un y va · demande ${ago}` : `Aide demandée ${ago}`;
        li.querySelector('.crew-alert-note').textContent = a.note ? `« ${a.note} »` : '';
        const meta = [];
        if (a.bus_m != null) meta.push(`à ${fmtDistance(a.bus_m)} de la navette`);
        meta.push(`à ${fmtDistance(a.route_m)} du trajet`);
        if (a.accuracy != null) meta.push(`± ${Math.round(a.accuracy)} m`);
        li.querySelector('.crew-alert-meta').textContent = meta.join(' · ');
        li.querySelector('.crew-alert-go').hidden = a.seen != null;
        li.querySelector('.crew-alert-map').href = `https://www.google.com/maps/dir/?api=1&destination=${a.lat},${a.lon}`;
        list.append(li);
      }
      q('.crew-alerts').hidden = alerts.length === 0;

      const waiting = alerts.some((a) => a.seen == null);
      if (waiting && !alarmTimer) {
        alarm();
        alarmTimer = setInterval(alarm, ALARM_MS);
      } else if (!waiting && alarmTimer) {
        clearInterval(alarmTimer);
        alarmTimer = 0;
      }
    }

    function render(data) {
      renderAlerts(data);
      const snap = data.snapshot;
      capacity = snap.capacity;
      for (const e of snap.etas) names.set(e.stop, e.name);
      if (Date.now() > holdUntil) count = snap.passengers;
      drawCount();

      const btn = q('.crew-depart');
      const waiting = snap.status === 'at_stop';
      btn.classList.toggle('ready', waiting);
      btn.textContent = snap.status === 'departing' ? 'Départ annoncé' : 'On part';
      const dep = snap.departure;
      const stop = (id) => names.get(id) ?? id;
      q('.crew-depart-note').textContent = waiting
        ? `À quai · ${stop(snap.at_stop)}. La page publique affiche « part dès qu'elle est pleine ».`
        : dep
          ? `Dernier départ annoncé à ${TIME_FMT.format(dep.ts)}${dep.stop ? ` (${stop(dep.stop)})` : ''}.`
          : '';

      // Lignes mises à jour sur place : un appui sur « Pris » pendant un
      // rafraîchissement ne doit pas tomber sur un bouton qui vient d'être remplacé.
      const now = Date.now();
      let fresh = false;
      const list = q('.crew-pickups ol');
      const ids = new Set(data.pickups.map((p) => p.id));
      for (const [id, li] of rows) {
        if (!ids.has(id)) {
          li.remove();
          rows.delete(id);
        }
      }
      for (const p of data.pickups) {
        if (!seen.has(p.id)) {
          seen.set(p.id, now);
          if (!firstLoad) fresh = true;
        }
        let li = rows.get(p.id);
        if (!li) {
          li = el('li', 'crew-pickup');
          const main = el('div', 'crew-pickup-main');
          main.append(el('strong'), el('span'));
          li.append(main);
          for (const [action, label] of [['picked', 'Pris'], ['dismissed', 'Ignorer']]) {
            const b = el('button', `crew-${action}`, label);
            b.type = 'button';
            b.dataset.id = p.id;
            b.dataset.action = action;
            li.append(b);
          }
          rows.set(p.id, li);
        }
        li.classList.toggle('new', now - seen.get(p.id) < NEW_MS);
        Object.assign(li.querySelector('.crew-pickup-main').dataset, { lat: p.lat, lon: p.lon });
        li.querySelector('strong').textContent = `${p.count} pers. → ${p.dest_name}`;
        li.querySelector('span').textContent = fmtAhead(p);
        list.append(li); // déplace sans recréer : l'ordre suit la distance
      }
      q('.crew-empty').hidden = data.pickups.length > 0;
      if (fresh) {
        navigator.vibrate?.([250, 120, 250]);
        beep();
      }
      firstLoad = false;
      renderMap(mapState, data);

      for (const b of root.querySelectorAll('[data-mode]')) b.setAttribute('aria-pressed', String(b.dataset.mode === snap.service));
      if (Date.now() > noteUntil) {
        q('.crew-current').textContent = snap.message ? `Affiché actuellement : « ${snap.message.text} »` : 'Aucun message affiché.';
      }
    }

    /** Message d'erreur à la place de « Affiché actuellement », le temps de le lire. */
    function fail(text) {
      noteUntil = Date.now() + 6000;
      q('.crew-current').textContent = text;
    }

    async function send(button, path, body) {
      button.disabled = true;
      try {
        await api(path, body);
        noteUntil = 0;
      } catch {
        fail('Pas envoyé : vérifie le réseau et réessaie.');
      }
      button.disabled = false;
      load();
    }

    async function load() {
      try {
        const data = await api('/api/crew/state');
        render(data);
        onState?.(data);
      } catch {
        onState?.(null);
      }
    }

    root.addEventListener('click', async (e) => {
      // Une alerte ou un signe touché (hors boutons) : on le montre sur la carte.
      const spot = e.target.closest('[data-lat]');
      if (spot && !e.target.closest('button, a')) {
        focusMap(mapState, Number(spot.dataset.lat), Number(spot.dataset.lon));
        return;
      }
      const b = e.target.closest('button');
      if (!b) return;
      if (b.dataset.delta) setCount((count ?? 0) + Number(b.dataset.delta));
      else if (b.dataset.set) setCount(b.dataset.set === 'full' ? capacity : 0);
      else if (b.classList.contains('crew-depart')) {
        b.disabled = true;
        await api('/api/crew/depart', {}).catch(() => {});
        b.disabled = false;
        load();
      } else if (b.dataset.action) {
        b.disabled = true;
        await api('/api/crew/pickup', { id: b.dataset.id, action: b.dataset.action }).catch(() => {});
        if (b.dataset.action === 'picked') holdUntil = 0; // le serveur a ajouté les personnes
        load();
      } else if (b.dataset.alertAction) {
        if (b.dataset.alertAction === 'closed' && !armed(b, 'Confirmer ?')) return;
        send(b, '/api/crew/help', { id: b.dataset.alert, action: b.dataset.alertAction });
      } else if (b.dataset.mode) {
        if (b.dataset.mode === 'off' && !armed(b, 'Confirmer ?')) return;
        send(b, '/api/crew/service', { mode: b.dataset.mode });
      } else if (b.hasAttribute('data-preset')) {
        q('.crew-msg').value = b.textContent;
      } else if (b.classList.contains('crew-publish')) {
        const text = q('.crew-msg').value.trim();
        if (!text) {
          fail('Écris un message ou choisis-en un ci-dessus.');
          return;
        }
        send(b, '/api/crew/message', { text, level: q('.crew-level').value });
      } else if (b.classList.contains('crew-clear')) {
        q('.crew-msg').value = '';
        send(b, '/api/crew/message', { text: '' });
      }
    });

    load();
    const timer = setInterval(() => document.hidden || load(), POLL_MS);
    return {
      reload: load,
      stop: () => {
        clearInterval(timer);
        clearInterval(alarmTimer);
        alarmTimer = 0;
      },
    };
  };
})();

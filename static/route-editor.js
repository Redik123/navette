'use strict';

// Éditeur du trajet (page orga) : les arrêts (carrés de couleur) et les points
// de passage de l'aller (carrés blancs numérotés), que l'on déplace, ajoute ou
// retire sur la carte. Le serveur recalcule l'itinéraire routier (OSRM), avec
// le retour par le même chemin, et le publie à tous.

(() => {
  const TILE_URL = 'https://wmts.geo.admin.ch/1.0.0/ch.swisstopo.pixelkarte-grau/default/current/3857/{z}/{x}/{y}.jpeg';
  const STOP_COLORS = ['#2f66d8', '#f2c300', '#e2321f', '#f2f1ec'];

  const round6 = (x) => Math.round(x * 1e6) / 1e6;
  const fmtKm = (m) => `${(m / 1000).toFixed(1).replace('.', ',')} km`;
  const copyStops = (list) => list.map((s) => ({ ...s }));

  /**
   * Monte l'éditeur dans #route-editor.
   * `getToken()` : jeton orga ; `onUnauthorized()` : jeton refusé.
   */
  window.mountRouteEditor = ({ getToken, onUnauthorized }) => {
    const $ = (id) => document.getElementById(id);
    if (!window.L) {
      $('route-editor').hidden = true;
      return null;
    }

    const map = L.map('route-map', { attributionControl: false, zoomControl: false });
    L.control.zoom({ position: 'topright' }).addTo(map);
    L.control.attribution({ prefix: false, position: 'bottomleft' }).addTo(map);
    L.tileLayer(TILE_URL, { minZoom: 11, maxNativeZoom: 18, maxZoom: 19, attribution: '© swisstopo' }).addTo(map);
    const current = L.polyline([], { color: '#f2f1ec', weight: 3, opacity: 0.8, interactive: false }).addTo(map);
    const preview = L.polyline([], { color: '#f2c300', weight: 4, dashArray: '8 6', interactive: false }).addTo(map);
    const guide = L.polyline([], { color: '#a3a29b', weight: 1, dashArray: '2 6', interactive: false }).addTo(map);
    const stopLayer = L.layerGroup().addTo(map);
    const pointLayer = L.layerGroup().addTo(map);
    // Noms des arrêts sous les points : on doit toujours pouvoir les attraper.
    map.createPane('stop-labels').style.zIndex = 590;

    let stops = []; // arrêts en cours d'édition : { id, name, lat, lon, detail }
    let publishedStops = [];
    let via = []; // points de passage en cours d'édition
    let published = []; // ceux du trajet publié
    let lengthNow = 0;
    let fitted = false;

    new ResizeObserver(() => {
      map.invalidateSize();
      if (!fitted && current.getLatLngs().length) {
        map.fitBounds(current.getBounds(), { padding: [20, 20] });
        fitted = true;
      }
    }).observe($('route-map'));

    async function api(path, body) {
      const res = await fetch(path, {
        method: body === undefined ? 'GET' : 'POST',
        headers: { Authorization: `Bearer ${getToken()}`, ...(body !== undefined && { 'Content-Type': 'application/json' }) },
        body: body === undefined ? undefined : JSON.stringify(body),
        cache: 'no-store',
      });
      if (res.status === 401) {
        onUnauthorized?.();
        throw new Error('Jeton refusé.');
      }
      if (res.status === 422) throw new Error((await res.json()).error);
      if (!res.ok) throw new Error(`Erreur du serveur (${res.status}).`);
      return res.status === 204 ? null : res.json();
    }

    function status(text, cls = '') {
      $('route-status').textContent = text;
      $('route-status').className = `route-status ${cls}`;
    }

    const plan = () => ({ via, stops: stops.map(({ id, lat, lon, detail }) => ({ id, lat, lon, detail })) });
    const pos = (s) => [s.lat, s.lon];

    /** Arrêts (carrés de couleur) et points de passage (numérotés), tous déplaçables. */
    function draw() {
      stopLayer.clearLayers();
      stops.forEach((s, i) => {
        const marker = L.marker(pos(s), {
          draggable: true,
          keyboard: false,
          zIndexOffset: 500,
          title: `${s.name} : glisser pour déplacer l'arrêt`,
          icon: L.divIcon({ className: 'route-stop', html: `<span style="background:${STOP_COLORS[i % STOP_COLORS.length]}"></span>`, iconSize: [24, 24] }),
        }).bindTooltip(s.name, { permanent: true, direction: 'auto', className: 'crew-map-label', offset: [14, 0], pane: 'stop-labels' });
        marker.on('dragend', () => {
          const p = marker.getLatLng();
          Object.assign(stops[i], { lat: round6(p.lat), lon: round6(p.lng) });
          changed(`Arrêt ${s.name} déplacé : touche « Calculer » pour voir le nouveau trajet.`);
        });
        stopLayer.addLayer(marker);
      });

      pointLayer.clearLayers();
      via.forEach(([lat, lon], i) => {
        const marker = L.marker([lat, lon], {
          draggable: true,
          keyboard: false,
          title: `Point ${i + 1} : glisser pour le déplacer, toucher pour le retirer`,
          icon: L.divIcon({ className: 'route-point', html: `<span>${i + 1}</span>`, iconSize: [26, 26] }),
        });
        marker.on('dragend', () => {
          const p = marker.getLatLng();
          via[i] = [round6(p.lat), round6(p.lng)];
          changed();
        });
        marker.on('click', () => {
          via.splice(i, 1);
          changed();
        });
        pointLayer.addLayer(marker);
      });
      if (stops.length === 2) guide.setLatLngs([pos(stops[0]), ...via, pos(stops[1])]);
    }

    /** Précision affichée sous le nom de chaque arrêt (« Arrêt provisoire, côté nord »). */
    function drawStopFields() {
      const box = $('route-stops');
      box.replaceChildren();
      stops.forEach((s, i) => {
        const row = document.createElement('label');
        row.className = 'route-stop-row';
        const name = document.createElement('span');
        name.className = 'route-stop-name';
        name.style.setProperty('--c', STOP_COLORS[i % STOP_COLORS.length]);
        name.textContent = s.name;
        const input = document.createElement('input');
        input.type = 'text';
        input.maxLength = 80;
        input.value = s.detail ?? '';
        input.placeholder = 'Précision affichée (ex. : Arrêt provisoire, côté nord)';
        input.addEventListener('input', () => {
          stops[i].detail = input.value;
          changed('Précision modifiée : touche « Calculer », puis « Appliquer ».');
        });
        row.append(name, input);
        box.append(row);
      });
    }

    function changed(message = 'Modifié : touche « Calculer » pour voir le nouveau trajet.') {
      preview.setLatLngs([]);
      $('route-apply').disabled = true;
      draw();
      status(message, 'warn');
    }

    /** Un nouveau point s'insère là où il allonge le moins l'aller. */
    map.on('click', (e) => {
      if (stops.length !== 2) return;
      const p = [round6(e.latlng.lat), round6(e.latlng.lng)];
      const seq = [pos(stops[0]), ...via, pos(stops[1])];
      const d = (a, b) => L.latLng(a).distanceTo(b);
      let best = 0;
      let bestCost = Infinity;
      for (let k = 0; k < seq.length - 1; k++) {
        const cost = d(seq[k], p) + d(p, seq[k + 1]) - d(seq[k], seq[k + 1]);
        if (cost < bestCost) {
          bestCost = cost;
          best = k;
        }
      }
      via.splice(best, 0, p);
      changed();
    });

    async function load() {
      const [cfg, state] = await Promise.all([
        fetch('/api/config', { cache: 'no-store' }).then((r) => r.json()),
        api('/api/admin/route'),
      ]);
      current.setLatLngs(cfg.route);
      publishedStops = state.stops.map(({ id, name, lat, lon, detail }) => ({ id, name, lat, lon, detail }));
      stops = copyStops(publishedStops);
      published = state.via.map((p) => [...p]);
      via = published.map((p) => [...p]);
      lengthNow = state.length_m;
      preview.setLatLngs([]);
      $('route-apply').disabled = true;
      draw();
      drawStopFields();
      if (!fitted && $('route-map').clientWidth) {
        map.fitBounds(current.getBounds(), { padding: [20, 20] });
        fitted = true;
      }
      status(state.custom ? `Trajet modifié par l'orga (${fmtKm(lengthNow)}).` : `Trajet d'origine (${fmtKm(lengthNow)}).`);
    }

    // Confirmation sans fenêtre du navigateur : un premier appui arme le bouton.
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

    async function run(button, busy, task) {
      const label = button.textContent;
      button.disabled = true;
      button.textContent = busy;
      try {
        await task();
      } catch (err) {
        status(err.message, 'err');
      } finally {
        button.textContent = label;
        button.disabled = button.id === 'route-apply' && !preview.getLatLngs().length;
      }
    }

    $('route-preview').addEventListener('click', (e) =>
      run(e.currentTarget, 'Calcul…', async () => {
        const res = await api('/api/admin/route/preview', plan());
        preview.setLatLngs(res.route);
        // Les arrêts déplacés sont recalés sur la rue : on montre où ils tombent.
        for (const s of res.stops ?? []) {
          const mine = stops.find((x) => x.id === s.id);
          if (mine) Object.assign(mine, { lat: s.lat, lon: s.lon });
        }
        draw();
        if (res.problem) {
          status(res.problem, 'err');
          $('route-apply').disabled = true;
          return;
        }
        const diff = res.length_m - lengthNow;
        const delta = Math.abs(diff) < 50 ? 'même longueur' : `${diff > 0 ? '+' : '−'}${fmtKm(Math.abs(diff))}`;
        status(`Nouveau trajet en jaune : ${fmtKm(res.length_m)} (${delta}). Vérifie-le, puis « Appliquer ».`);
        $('route-apply').disabled = false;
      }),
    );

    $('route-apply').addEventListener('click', (e) => {
      const b = e.currentTarget;
      if (!armed(b, 'Confirmer : publier')) return;
      run(b, 'Publication…', async () => {
        await api('/api/admin/route', plan());
        preview.setLatLngs([]);
        await load();
        status('Nouveau trajet publié : les pages de tout le monde se mettent à jour.');
      });
    });

    $('route-undo').addEventListener('click', () => {
      stops = copyStops(publishedStops);
      via = published.map((p) => [...p]);
      preview.setLatLngs([]);
      $('route-apply').disabled = true;
      draw();
      drawStopFields();
      status('Changements annulés.');
    });

    $('route-reset').addEventListener('click', (e) => {
      const b = e.currentTarget;
      if (!armed(b, 'Confirmer : revenir')) return;
      run(b, 'Retour…', async () => {
        await api('/api/admin/route/reset', {});
        await load();
        status(`Trajet et arrêts d'origine rétablis (${fmtKm(lengthNow)}).`);
      });
    });

    load().catch((err) => status(err.message, 'err'));
    return { reload: () => load().catch((err) => status(err.message, 'err')) };
  };
})();

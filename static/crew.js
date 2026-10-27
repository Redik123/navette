'use strict';

// Commandes de l'équipage, partagées par la page chauffeur et la page orga :
// personnes à bord, annonce du départ, signes des gens qui attendent sur le
// trajet. Toute la logique (arrivée, remise à zéro, passage) est côté serveur.

(() => {
  const POLL_MS = 3000;
  /** Après un appui, le compteur local fait foi le temps que le serveur suive. */
  const LOCAL_HOLD_MS = 4000;
  /** Un nouveau signe reste en surbrillance… */
  const NEW_MS = 20_000;

  const TIME_FMT = new Intl.DateTimeFormat('fr-CH', { hour: '2-digit', minute: '2-digit', timeZone: 'Europe/Zurich' });

  const TEMPLATE = `
    <section class="crew-seats">
      <div class="crew-label">Personnes à bord</div>
      <div class="crew-counter">
        <button type="button" class="crew-step" data-delta="-1" aria-label="Une personne de moins">−</button>
        <output class="crew-count" aria-live="polite">—</output>
        <button type="button" class="crew-step" data-delta="1" aria-label="Une personne de plus">+</button>
      </div>
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
    <p class="crew-note">À manipuler à l'arrêt, jamais en roulant. Le compteur repart de zéro à chaque arrêt.</p>`;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function fmtAhead(p) {
    if (p.distance_m == null) return 'navette pas localisée';
    const d = p.distance_m < 1000 ? `${Math.round(p.distance_m / 10) * 10} m` : `${(p.distance_m / 1000).toFixed(1).replace('.', ',')} km`;
    return `dans ${d} · ~${Math.max(1, Math.round(p.eta_s / 60))} min`;
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
    let capacity = 16;
    let count = null;
    let holdUntil = 0;
    let sendTimer = 0;
    let names = new Map();
    const seen = new Map(); // id → première apparition
    const rows = new Map(); // id → ligne affichée
    let firstLoad = true;

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
      q('.crew-free').classList.toggle('full', free != null && free <= 0);
    }

    function setCount(n) {
      count = Math.max(0, Math.min(capacity, n));
      holdUntil = Date.now() + LOCAL_HOLD_MS;
      drawCount();
      clearTimeout(sendTimer);
      sendTimer = setTimeout(() => api('/api/crew/passengers', { passengers: count }).catch(() => {}), 300);
    }

    function render(data) {
      const snap = data.snapshot;
      capacity = snap.capacity;
      for (const e of snap.etas) names.set(e.stop, e.name);
      if (Date.now() > holdUntil) count = snap.passengers;
      drawCount();

      const btn = q('.crew-depart');
      const waiting = snap.status === 'at_stop';
      btn.classList.toggle('ready', waiting);
      btn.textContent = snap.status === 'departing' ? 'Départ annoncé ✓' : 'On part';
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
      }
    });

    load();
    const timer = setInterval(() => document.hidden || load(), POLL_MS);
    return { reload: load, stop: () => clearInterval(timer) };
  };
})();

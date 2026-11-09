<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import L from 'leaflet';
  import {
    INITIAL_CENTER,
    INITIAL_ZOOM,
    MAX_ZOOM,
    TILE_SIZE,
    botwBounds,
    createBotwCrs,
  } from '../lib/coords';
  import {
    getMapCompletion,
    getMapObjects,
    type CompletedObjects,
    type MapObject,
    type MapObjectType,
  } from '../lib/api';
  import {
    filterObjects,
    isComplete,
    nearestUnfinished,
    type CompletionFilter,
  } from '../lib/mapObjects';

  let {
    mapAssets,
    slot,
    position,
  }: {
    mapAssets: boolean;
    slot: number | null;
    position: { x: number; z: number } | null;
  } = $props();

  interface CategoryStyle {
    icon: string;
    width: number;
    height: number;
    color: string;
    label: string;
  }

  const OBJECT_TYPES = [
    'shrine',
    'tower',
    'memory',
    'korok',
    'location',
    'enemy',
    'treasure',
    'divineBeast',
  ] as const;

  const STYLES: Record<MapObjectType, CategoryStyle> = {
    shrine: {
      icon: 'shrine',
      width: 26,
      height: 27,
      color: '#5ddbea',
      label: 'Shrines',
    },
    tower: {
      icon: 'tower',
      width: 28,
      height: 39,
      color: '#e3bd6d',
      label: 'Towers',
    },
    memory: {
      icon: 'memory',
      width: 30,
      height: 30,
      color: '#b7a7f5',
      label: 'Memories',
    },
    korok: {
      icon: 'seed',
      width: 18,
      height: 18,
      color: '#a8e65d',
      label: 'Korok Seeds',
    },
    location: {
      icon: 'settlement',
      width: 22,
      height: 22,
      color: '#f5d75d',
      label: 'Locations',
    },
    enemy: {
      icon: 'hinox',
      width: 24,
      height: 24,
      color: '#ff6b6b',
      label: 'Enemies',
    },
    treasure: {
      icon: 'treasure',
      width: 30,
      height: 26,
      color: '#e08b3c',
      label: 'Chests',
    },
    divineBeast: {
      icon: 'mainquest',
      width: 34,
      height: 20,
      color: '#d98cff',
      label: 'Divine Beasts',
    },
  };

  /** Dense layers cluster; landmarks stay as individual pins. */
  const CLUSTERED: ReadonlySet<MapObjectType> = new Set<MapObjectType>([
    'korok',
    'treasure',
    'enemy',
    'location',
    'memory',
  ]);

  interface TypeState {
    objects: MapObject[];
    markers: { object: MapObject; marker: L.Marker }[];
    group: L.LayerGroup | L.MarkerClusterGroup;
  }

  let mapEl: HTMLDivElement | undefined = $state();
  let map: L.Map | null = null;
  let loadError = $state<string | null>(null);
  let completionError = $state<string | null>(null);
  let showShrines = $state(true);
  let showTowers = $state(true);
  let showMemories = $state(false);
  let showKoroks = $state(false);
  let showLocations = $state(false);
  let showEnemies = $state(false);
  let showTreasures = $state(false);
  let showDivineBeasts = $state(true);
  let filter = $state<CompletionFilter>('all');
  let completedObjects = $state<CompletedObjects | null>(null);
  let counts = $state<Record<MapObjectType, number>>(emptyCounts());
  let progress =
    $state<Record<MapObjectType, { completed: number; total: number }>>(emptyProgress());
  let overall = $state({ completed: 0, total: 0 });

  const typeStates: Partial<Record<MapObjectType, TypeState>> = {};
  let completionRequest = 0;

  const overallPercentage = $derived(
    overall.total > 0 ? Math.round((overall.completed / overall.total) * 100) : 0,
  );

  function emptyCounts(): Record<MapObjectType, number> {
    return {
      shrine: 0,
      tower: 0,
      memory: 0,
      korok: 0,
      location: 0,
      enemy: 0,
      treasure: 0,
      divineBeast: 0,
    };
  }

  function emptyProgress(): Record<MapObjectType, { completed: number; total: number }> {
    return {
      shrine: { completed: 0, total: 0 },
      tower: { completed: 0, total: 0 },
      memory: { completed: 0, total: 0 },
      korok: { completed: 0, total: 0 },
      location: { completed: 0, total: 0 },
      enemy: { completed: 0, total: 0 },
      treasure: { completed: 0, total: 0 },
      divineBeast: { completed: 0, total: 0 },
    };
  }

  // Fetch per-object completion whenever the selected slot changes. The
  // request counter drops responses for a slot the user has already left.
  $effect(() => {
    const current = slot;
    if (current === null) {
      completionRequest += 1;
      completedObjects = null;
      completionError = null;
      return;
    }
    void loadCompletion(current);
  });

  // Recompute the progress panel and redraw markers when completion or the
  // filter changes. Layer visibility is handled by syncLayers on toggle.
  $effect(() => {
    const completion = completedObjects;
    const activeFilter = filter;
    untrack(() => {
      refreshDerived(completion);
      renderMarkers(activeFilter, completion);
      syncLayers();
    });
  });

  onMount(() => {
    const el = mapEl;
    if (!el) return;

    const crs = createBotwCrs();
    const bounds = botwBounds(crs);
    const instance = L.map(el, {
      crs,
      minZoom: 0,
      maxZoom: MAX_ZOOM,
      zoomSnap: 0.5,
      zoomDelta: 0.5,
      zoomControl: false,
      maxBounds: bounds.pad(0.5),
      attributionControl: false,
      preferCanvas: true,
    });
    instance.setView(INITIAL_CENTER, INITIAL_ZOOM);

    // Keep Leaflet's cached size in sync with the container. Without this the
    // map is only measured once on mount and tiles stop at the initial height
    // after viewport chrome, orientation, or the mobile layout changes.
    const resizeObserver = new ResizeObserver(() => instance.invalidateSize());
    resizeObserver.observe(el);

    L.control.zoom({ position: 'bottomright' }).addTo(instance);
    L.control
      .attribution({ position: 'bottomleft', prefix: false })
      .addTo(instance)
      .addAttribution(
        'Tiles &amp; marker data © <a href="https://www.zeldadungeon.net/breath-of-the-wild-interactive-map">Zelda Dungeon</a> · Wayfinder Slate',
      );

    map = instance;

    if (mapAssets) {
      L.tileLayer('/map/botw/tiles/{z}/{x}_{y}.jpg', {
        tileSize: TILE_SIZE,
        minZoom: 0,
        maxZoom: MAX_ZOOM,
        bounds,
        noWrap: true,
        attribution: '',
      }).addTo(instance);
      void loadMapObjects();
    }

    return () => {
      resizeObserver.disconnect();
      instance.remove();
      map = null;
    };
  });

  async function loadCompletion(current: number) {
    const request = ++completionRequest;
    try {
      const result = await getMapCompletion(current);
      if (request !== completionRequest) return;
      completedObjects = result.completed;
      completionError = null;
    } catch (cause) {
      if (request !== completionRequest) return;
      completedObjects = null;
      completionError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function loadMapObjects() {
    try {
      const dataset = await getMapObjects();
      const datasetMap: Record<MapObjectType, MapObject[]> = {
        shrine: dataset.shrines,
        tower: dataset.towers,
        memory: dataset.memories,
        korok: dataset.koroks,
        location: dataset.locations,
        enemy: dataset.enemies,
        treasure: dataset.treasures,
        divineBeast: dataset.divineBeasts,
      };

      for (const type of OBJECT_TYPES) {
        const objects = datasetMap[type];
        const style = STYLES[type];
        if (objects.length === 0) continue;

        const icon = L.icon({
          iconUrl: `/map/botw/icons/${style.icon}.png`,
          iconSize: [style.width, style.height],
          iconAnchor: [style.width / 2, style.height],
          popupAnchor: [0, -style.height + 5],
        });

        const group = CLUSTERED.has(type)
          ? L.markerClusterGroup({
              maxClusterRadius: 60,
              showCoverageOnHover: false,
              spiderfyOnMaxZoom: true,
              disableClusteringAtZoom: 4,
            })
          : L.layerGroup();

        const markers = objects.map((object) => {
          const name = markerName(type, object);
          const detail = markerDetail(type, object);
          const marker = L.marker([-object.z, object.x], { icon, title: name }).bindPopup(
            `<strong>${escapeHtml(name)}</strong>` +
              (detail ? `<br><span style="opacity:.65">${escapeHtml(detail)}</span>` : '') +
              (object.dlc ? '<br><span style="opacity:.65">DLC</span>' : ''),
          );
          return { object, marker };
        });

        typeStates[type] = { objects, markers, group };
      }

      refreshDerived(completedObjects);
      renderMarkers(filter, completedObjects);
      syncLayers();
    } catch (cause) {
      loadError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  /** Recompute counts and progress from the dataset and the completion response. */
  function refreshDerived(completion: CompletedObjects | null) {
    const nextCounts = emptyCounts();
    const nextProgress = emptyProgress();
    let completedTotal = 0;
    let total = 0;

    for (const type of OBJECT_TYPES) {
      const objects = typeStates[type]?.objects ?? [];
      let completedCount = 0;
      if (completion !== null) {
        for (const object of objects) {
          if (isComplete(type, object, completion)) completedCount += 1;
        }
      }
      nextCounts[type] = objects.length;
      nextProgress[type] = { completed: completedCount, total: objects.length };
      completedTotal += completedCount;
      total += objects.length;
    }

    counts = nextCounts;
    progress = nextProgress;
    overall = { completed: completedTotal, total };
  }

  /** Clear and repopulate each layer with the objects that pass the filter. */
  function renderMarkers(activeFilter: CompletionFilter, completion: CompletedObjects | null) {
    for (const type of OBJECT_TYPES) {
      const state = typeStates[type];
      if (!state) continue;
      state.group.clearLayers();
      const visible = new Set(filterObjects(type, state.objects, completion, activeFilter));
      for (const entry of state.markers) {
        if (visible.has(entry.object)) state.group.addLayer(entry.marker);
      }
    }
  }

  function markerName(type: MapObjectType, object: MapObject): string {
    switch (type) {
      case 'shrine':
      case 'tower':
      case 'memory':
      case 'divineBeast':
        return object.name ?? object.flag ?? 'Unknown';
      case 'korok':
        return object.seed
          ? `Korok Seed ${object.seed.replace(/^Seed/, '')}`
          : (object.loc ?? object.flag ?? 'Unknown Korok');
      case 'location':
        return object.match_name ?? object.flag ?? 'Unknown location';
      case 'enemy':
        return object.type ?? 'Enemy';
      case 'treasure':
        return object.name ?? 'Treasure';
    }
  }

  /// Secondary line shown under a marker's title in its popup.
  function markerDetail(type: MapObjectType, object: MapObject): string | null {
    if (type === 'korok') return object.loc ?? null;
    if (type === 'treasure') return object.group ?? null;
    return null;
  }

  function getVisibility(type: MapObjectType): boolean {
    switch (type) {
      case 'shrine':
        return showShrines;
      case 'tower':
        return showTowers;
      case 'memory':
        return showMemories;
      case 'korok':
        return showKoroks;
      case 'location':
        return showLocations;
      case 'enemy':
        return showEnemies;
      case 'treasure':
        return showTreasures;
      case 'divineBeast':
        return showDivineBeasts;
    }
  }

  function syncLayers() {
    if (!map) return;
    for (const type of OBJECT_TYPES) {
      const group = typeStates[type]?.group;
      if (!group) continue;
      const visible = getVisibility(type);
      if (visible && !map.hasLayer(group)) group.addTo(map);
      if (!visible && map.hasLayer(group)) map.removeLayer(group);
    }
  }

  function setFilter(next: CompletionFilter) {
    if (filter === next) return;
    filter = next;
  }

  /** Fly to the closest unfinished object and open its popup. */
  function focusNearestUnfinished() {
    if (!map) return;

    const center = map.getCenter();
    const origin = position ?? { x: center.lng, z: -center.lat };

    let nearest: { type: MapObjectType; object: MapObject } | null = null;
    let nearestDistance = Number.POSITIVE_INFINITY;

    for (const type of OBJECT_TYPES) {
      if (!getVisibility(type)) continue;
      const state = typeStates[type];
      if (!state) continue;

      const object = nearestUnfinished(type, state.objects, completedObjects, origin);
      if (object === null) continue;

      const dx = object.x - origin.x;
      const dz = object.z - origin.z;
      const distance = dx * dx + dz * dz;
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearest = { type, object };
      }
    }

    if (nearest === null) return;
    const target = nearest;

    // The completed-only filter would hide an unfinished target; switch to the
    // unfinished view so its marker is present before opening the popup.
    if (filter === 'completed') {
      filter = 'unfinished';
      renderMarkers(filter, completedObjects);
      syncLayers();
    }

    map.flyTo([-target.object.z, target.object.x], Math.max(map.getZoom(), 4), {
      duration: 0.8,
    });

    const marker = typeStates[target.type]?.markers.find(
      (entry) => entry.object === target.object,
    )?.marker;
    if (marker) marker.openPopup();
  }

  function progressPercentage(type: MapObjectType): number {
    const entry = progress[type];
    return entry.total > 0 ? (entry.completed / entry.total) * 100 : 0;
  }

  function escapeHtml(value: string): string {
    return value.replace(
      /[&<>"']/g,
      (character) =>
        ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[
          character
        ] ?? character,
    );
  }
</script>

<div class="map-shell">
  <div class="map-canvas" bind:this={mapEl}></div>

  {#if !mapAssets}
    <div class="map-empty">
      <div class="map-empty-card">
        <span class="eyebrow">Map assets missing</span>
        <h2>The map has not been installed yet.</h2>
        <p>
          Fetch the attributed Breath of the Wild tiles and marker data, then
          restart the server:
        </p>
        <pre class="command">./scripts/fetch-map-assets.sh
npm --prefix frontend run build</pre>
      </div>
    </div>
  {/if}

  {#if loadError !== null}
    <div class="map-error" role="status">Marker data: {loadError}</div>
  {/if}

  <div class="map-panel">
    <div class="panel-title">Layers</div>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showTowers} onchange={syncLayers} />
      <span class="swatch" style="background: #e3bd6d"></span>
      <span>Towers</span>
      <span class="count">{counts.tower || '—'}</span>
    </label>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showShrines} onchange={syncLayers} />
      <span class="swatch" style="background: #5ddbea"></span>
      <span>Shrines</span>
      <span class="count">{counts.shrine || '—'}</span>
    </label>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showDivineBeasts} onchange={syncLayers} />
      <span class="swatch" style="background: #d98cff"></span>
      <span>Divine Beasts</span>
      <span class="count">{counts.divineBeast || '—'}</span>
    </label>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showMemories} onchange={syncLayers} />
      <span class="swatch" style="background: #b7a7f5"></span>
      <span>Memories</span>
      <span class="count">{counts.memory || '—'}</span>
    </label>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showKoroks} onchange={syncLayers} />
      <span class="swatch" style="background: #a8e65d"></span>
      <span>Koroks</span>
      <span class="count">{counts.korok || '—'}</span>
    </label>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showLocations} onchange={syncLayers} />
      <span class="swatch" style="background: #f5d75d"></span>
      <span>Locations</span>
      <span class="count">{counts.location || '—'}</span>
    </label>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showEnemies} onchange={syncLayers} />
      <span class="swatch" style="background: #ff6b6b"></span>
      <span>Enemies</span>
      <span class="count">{counts.enemy || '—'}</span>
    </label>

    <label class="layer-toggle">
      <input type="checkbox" bind:checked={showTreasures} onchange={syncLayers} />
      <span class="swatch" style="background: #e08b3c"></span>
      <span>Chests</span>
      <span class="count">{counts.treasure || '—'}</span>
    </label>

    <div class="panel-divider"></div>

    <div class="completion-block">
      <div class="panel-title">
        Completion
        <span class="overall-pct">{overallPercentage}%</span>
      </div>
      <div class="overall-bar" aria-hidden="true">
        <span style={`width: ${overallPercentage}%`}></span>
      </div>

      {#each OBJECT_TYPES as type (type)}
        <div class="completion-row">
          <span class="swatch" style={`background: ${STYLES[type].color}`}></span>
          <span>{STYLES[type].label}</span>
          <span class="count">{progress[type].completed}/{progress[type].total}</span>
        </div>
        <div class="progress-bar" aria-hidden="true">
          <span
            style={`width: ${progressPercentage(type)}%; background: ${STYLES[type].color}`}
          ></span>
        </div>
      {/each}
    </div>

    <div class="panel-divider"></div>

    <div class="filter-block">
      <div class="filter-control" role="group" aria-label="Completion filter">
        <button
          type="button"
          class:active={filter === 'all'}
          aria-pressed={filter === 'all'}
          onclick={() => setFilter('all')}
        >
          All
        </button>
        <button
          type="button"
          class:active={filter === 'unfinished'}
          aria-pressed={filter === 'unfinished'}
          onclick={() => setFilter('unfinished')}
        >
          Unfinished
        </button>
        <button
          type="button"
          class:active={filter === 'completed'}
          aria-pressed={filter === 'completed'}
          onclick={() => setFilter('completed')}
        >
          Completed
        </button>
      </div>

      <button type="button" class="nearest-button" onclick={focusNearestUnfinished}>
        Nearest unfinished
      </button>
    </div>

    {#if completionError !== null}
      <p class="panel-note panel-error">{completionError}</p>
    {/if}

    <p class="panel-note">
      Completion reflects the selected save slot and updates when you switch
      slots. The map uses x/z; height is shown separately when live telemetry is
      connected.
    </p>
  </div>

  <div class="map-status">
    <span class="dot" class:online={position !== null}></span>
    {#if position}
      Last saved position {Math.round(position.x)}, {Math.round(position.z)}
    {:else}
      No saved position for this slot
    {/if}
  </div>
</div>

<style>
  .map-shell {
    position: relative;
    height: 100%;
    min-height: 420px;
    background: #071318;
  }

  .map-canvas {
    position: absolute;
    inset: 0;
    background-color: #071318;
    background-image:
      linear-gradient(rgba(93, 219, 234, 0.035) 1px, transparent 1px),
      linear-gradient(90deg, rgba(93, 219, 234, 0.035) 1px, transparent 1px);
    background-size: 46px 46px;
  }

  .map-panel {
    position: absolute;
    top: 16px;
    right: 16px;
    z-index: 500;
    width: 232px;
    max-height: calc(100% - 32px);
    padding: 14px 15px;
    border: 1px solid rgba(42, 86, 99, 0.7);
    border-radius: 12px;
    background: rgba(7, 20, 26, 0.9);
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.34);
    backdrop-filter: blur(9px);
    overflow-y: auto;
  }

  .panel-title {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 10px;
    color: var(--muted);
    font-size: 10px;
    letter-spacing: 0.17em;
    text-transform: uppercase;
  }

  .overall-pct {
    color: var(--cyan);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .layer-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
    color: var(--text-soft);
    font-size: 12.5px;
    cursor: pointer;
  }

  .layer-toggle input {
    appearance: none;
    width: 13px;
    height: 13px;
    margin: 0;
    border: 1px solid var(--line-bright);
    border-radius: 3px;
    background: transparent;
    display: grid;
    place-items: center;
    cursor: pointer;
  }

  .layer-toggle input:checked {
    border-color: var(--cyan);
    background: rgba(93, 219, 234, 0.18);
  }

  .layer-toggle input:checked::after {
    content: '';
    width: 6px;
    height: 6px;
    border-radius: 1px;
    background: var(--cyan);
  }

  .swatch {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    box-shadow: 0 0 7px currentColor;
  }

  .count {
    margin-left: auto;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
  }

  .panel-divider {
    height: 1px;
    margin: 11px 0;
    background: var(--line);
  }

  .overall-bar {
    height: 5px;
    margin-bottom: 10px;
    border-radius: 999px;
    background: rgba(93, 219, 234, 0.12);
    overflow: hidden;
  }

  .overall-bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--cyan);
    transition: width 0.2s ease;
  }

  .completion-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--text-soft);
  }

  .completion-row .count {
    margin-left: auto;
    font-weight: 600;
    color: var(--text);
  }

  .progress-bar {
    height: 3px;
    margin: 3px 0 6px 13px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.06);
    overflow: hidden;
  }

  .progress-bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    transition: width 0.2s ease;
  }

  .filter-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .filter-control {
    display: flex;
    border: 1px solid var(--line-bright);
    border-radius: 8px;
    overflow: hidden;
  }

  .filter-control button {
    flex: 1;
    padding: 6px 4px;
    border: 0;
    background: transparent;
    color: var(--muted);
    font-size: 10.5px;
    cursor: pointer;
  }

  .filter-control button + button {
    border-left: 1px solid var(--line);
  }

  .filter-control button.active {
    background: rgba(93, 219, 234, 0.16);
    color: var(--text);
  }

  .filter-control button:focus-visible,
  .nearest-button:focus-visible {
    outline: 2px solid var(--cyan);
    outline-offset: 1px;
  }

  .nearest-button {
    padding: 7px 10px;
    border: 1px solid var(--line-bright);
    border-radius: 8px;
    background: transparent;
    color: var(--text-soft);
    font-size: 11.5px;
    cursor: pointer;
  }

  .nearest-button:hover {
    border-color: var(--cyan);
    color: var(--text);
  }

  .panel-note {
    margin-top: 10px;
    color: var(--muted);
    font-size: 11px;
    line-height: 1.45;
  }

  .panel-error {
    color: #f0a0a0;
  }

  .map-status {
    position: absolute;
    left: 50%;
    bottom: 18px;
    z-index: 500;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: rgba(7, 20, 26, 0.9);
    color: var(--muted);
    font-size: 11.5px;
    white-space: nowrap;
    backdrop-filter: blur(8px);
  }

  .map-empty {
    position: absolute;
    inset: 0;
    z-index: 400;
    display: grid;
    place-items: center;
    padding: 24px;
    pointer-events: none;
  }

  .map-empty-card {
    max-width: 480px;
    padding: 26px 28px;
    border: 1px solid var(--line-bright);
    border-radius: var(--radius);
    background: rgba(7, 20, 26, 0.94);
    box-shadow: var(--shadow);
    pointer-events: auto;
  }

  .map-empty-card h2 {
    margin: 7px 0 8px;
    font-size: 24px;
  }

  .map-empty-card p {
    color: var(--text-soft);
    font-size: 13px;
  }

  .map-error {
    position: absolute;
    top: 16px;
    left: 50%;
    z-index: 600;
    transform: translateX(-50%);
    padding: 8px 13px;
    border: 1px solid rgba(240, 139, 139, 0.4);
    border-radius: 8px;
    background: rgba(50, 19, 22, 0.95);
    color: #ffd9d9;
    font-size: 12px;
  }

  @media (max-width: 820px) {
    .map-panel {
      top: auto;
      right: 10px;
      bottom: 52px;
      left: 10px;
      width: auto;
      max-height: 46vh;
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0 16px;
      padding: 9px 12px;
    }

    .panel-title,
    .panel-divider,
    .panel-note,
    .completion-block {
      display: none;
    }

    .layer-toggle {
      padding: 4px 0;
    }

    .filter-block {
      flex-direction: row;
      align-items: center;
      gap: 8px;
    }

    .nearest-button {
      white-space: nowrap;
    }

    .map-status {
      bottom: 12px;
    }
  }
</style>
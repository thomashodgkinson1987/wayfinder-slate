<script lang="ts">
  import { onMount } from 'svelte';
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
    getMapObjects,
    type CompletionSummary,
    type MapObject,
    type MapObjectType,
  } from '../lib/api';

  let {
    mapAssets,
    completion,
  }: {
    mapAssets: boolean;
    completion: CompletionSummary | null;
  } = $props();

  interface CategoryStyle {
    icon: string;
    width: number;
    height: number;
    color: string;
    label: string;
  }

  const OBJECT_TYPES = ['shrine', 'tower', 'memory', 'korok', 'location', 'enemy'] as const;

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
  };

  let mapEl: HTMLDivElement | undefined = $state();
  let map: L.Map | null = null;
  let loadError = $state<string | null>(null);
  let showShrines = $state(true);
  let showTowers = $state(true);
  let showMemories = $state(false);
  let showKoroks = $state(false);
  let showLocations = $state(false);
  let showEnemies = $state(false);
  let counts = $state<Record<MapObjectType, number>>({
    shrine: 0,
    tower: 0,
    memory: 0,
    korok: 0,
    location: 0,
    enemy: 0,
  });

  const groups: Partial<Record<MapObjectType, L.LayerGroup>> = {};

  onMount(() => {
    if (!mapEl) return;

    const crs = createBotwCrs();
    const bounds = botwBounds(crs);
    const instance = L.map(mapEl, {
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

    L.control.zoom({ position: 'bottomright' }).addTo(instance);
    L.control
      .attribution({ position: 'bottomleft', prefix: false })
      .addTo(instance)
      .addAttribution(
        'Tiles &amp; marker data © <a href="https://www.zeldadungeon.net/breath-of-the-wild-interactive-map">Zelda Dungeon</a> · Wayfinder Slate',
      );

    if (mapAssets) {
      L.tileLayer('/map/botw/tiles/{z}/{x}_{y}.jpg', {
        tileSize: TILE_SIZE,
        minZoom: 0,
        maxZoom: MAX_ZOOM,
        bounds,
        noWrap: true,
        attribution: '',
      }).addTo(instance);
      void loadMapObjects(instance);
    }

    map = instance;
    return () => {
      instance.remove();
      map = null;
    };
  });

  async function loadMapObjects(instance: L.Map) {
    try {
      const dataset = await getMapObjects();
      const datasetMap: Record<MapObjectType, MapObject[]> = {
        shrine: dataset.shrines,
        tower: dataset.towers,
        memory: dataset.memories,
        korok: dataset.koroks,
        location: dataset.locations,
        enemy: dataset.enemies,
      };

      for (const type of OBJECT_TYPES) {
        const objects = datasetMap[type];
        const style = STYLES[type];
        if (objects.length === 0) continue;

        const group = L.layerGroup();
        const icon = L.icon({
          iconUrl: `/map/botw/icons/${style.icon}.png`,
          iconSize: [style.width, style.height],
          iconAnchor: [style.width / 2, style.height],
          popupAnchor: [0, -style.height + 5],
        });

        for (const object of objects) {
          const name = markerName(type, object);
          const marker = L.marker([-object.z, object.x], { icon, title: name }).bindPopup(
            `<strong>${escapeHtml(name)}</strong>${
              object.dlc ? '<br><span style="opacity:.65">DLC</span>' : ''
            }`,
          );
          marker.addTo(group);
        }

        groups[type] = group;
        counts[type] = objects.length;
        if (getVisibility(type)) group.addTo(instance);
      }
    } catch (cause) {
      loadError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function markerName(type: MapObjectType, object: MapObject): string {
    switch (type) {
      case 'shrine':
      case 'tower':
      case 'memory':
        return object.name ?? object.flag;
      case 'korok':
        return object.loc ?? (object.seed ? `Korok Seed ${object.seed}` : object.flag);
      case 'location':
        return object.match_name ?? object.flag;
      case 'enemy':
        return object.type ?? 'Enemy';
    }
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
    }
  }

  function syncLayers() {
    if (!map) return;
    for (const type of OBJECT_TYPES) {
      const group = groups[type];
      if (!group) continue;
      const visible = getVisibility(type);
      if (visible && !map.hasLayer(group)) group.addTo(map);
      if (!visible && map.hasLayer(group)) map.removeLayer(group);
    }
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

    <div class="panel-divider"></div>

    {#if completion}
      <div class="completion-summary">
        <span class="completion-row">
          <span class="swatch" style="background: #5ddbea"></span>
          <span>Shrines</span>
          <span class="count">{completion.shrines.completed}/{completion.shrines.total}</span>
        </span>
        <span class="completion-row">
          <span class="swatch" style="background: #e3bd6d"></span>
          <span>Towers</span>
          <span class="count">{completion.towers.completed}/{completion.towers.total}</span>
        </span>
        <span class="completion-row">
          <span class="swatch" style="background: #b7a7f5"></span>
          <span>Memories</span>
          <span class="count">{completion.memories.completed}/{completion.memories.total}</span>
        </span>
        <span class="completion-row">
          <span class="swatch" style="background: #a8e65d"></span>
          <span>Koroks</span>
          <span class="count">{completion.koroks.completed}/{completion.koroks.total}</span>
        </span>
        <span class="completion-row">
          <span class="swatch" style="background: #f5d75d"></span>
          <span>Locations</span>
          <span class="count">{completion.locations.completed}/{completion.locations.total}</span>
        </span>
        <span class="completion-row">
          <span class="swatch" style="background: #ff6b6b"></span>
          <span>Enemies</span>
          <span class="count">{completion.enemies.completed}/{completion.enemies.total}</span>
        </span>
      </div>
    {/if}

    <div class="panel-divider"></div>

    <p class="panel-note">
      Completion reflects the selected save slot and updates when you switch
      slots. The map uses x/z; height is shown separately when live telemetry is
      connected.
    </p>
  </div>

  <div class="map-status">
    <span class="dot"></span>
    Last saved position will appear here
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
    width: 228px;
    padding: 14px 15px;
    border: 1px solid rgba(42, 86, 99, 0.7);
    border-radius: 12px;
    background: rgba(7, 20, 26, 0.9);
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.34);
    backdrop-filter: blur(9px);
  }

  .panel-title {
    margin-bottom: 10px;
    color: var(--muted);
    font-size: 10px;
    letter-spacing: 0.17em;
    text-transform: uppercase;
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

  .completion-summary {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 4px 0;
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

  .panel-note {
    color: var(--muted);
    font-size: 11px;
    line-height: 1.45;
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
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0 16px;
      padding: 9px 12px;
    }

    .panel-title,
    .panel-divider,
    .panel-note {
      display: none;
    }

    .layer-toggle {
      padding: 4px 0;
    }

    .completion-summary {
      display: none;
    }

    .map-status {
      bottom: 12px;
    }
  }
</style>

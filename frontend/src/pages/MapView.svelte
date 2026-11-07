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

  let { mapAssets }: { mapAssets: boolean } = $props();

  interface MarkerJson {
    coords: [number, number];
    id: string;
    name: string;
    tags?: string[];
  }

  interface LayerJson {
    markers: MarkerJson[];
  }

  interface CategoryJson {
    name: string;
    layers: LayerJson[];
  }

  interface CategoryStyle {
    icon: string;
    width: number;
    height: number;
    color: string;
    label: string;
  }

  const STYLES: Record<string, CategoryStyle> = {
    'Sheikah Tower': {
      icon: 'tower',
      width: 28,
      height: 39,
      color: '#e3bd6d',
      label: 'Towers',
    },
    Shrine: {
      icon: 'shrine',
      width: 26,
      height: 27,
      color: '#5ddbea',
      label: 'Shrines',
    },
    Memory: {
      icon: 'memory',
      width: 30,
      height: 30,
      color: '#b7a7f5',
      label: 'Memories',
    },
  };

  let mapEl: HTMLDivElement | undefined = $state();
  let map: L.Map | null = null;
  let loadError = $state<string | null>(null);
  let showTowers = $state(true);
  let showShrines = $state(true);
  let showMemories = $state(false);
  let counts = $state({ tower: 0, shrine: 0, memory: 0 });

  const groups: Partial<Record<string, L.LayerGroup>> = {};

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
      void loadMarkers(instance);
    }

    map = instance;
    return () => {
      instance.remove();
      map = null;
    };
  });

  async function loadMarkers(instance: L.Map) {
    try {
      const response = await fetch('/map/botw/markers/pins.json');
      if (!response.ok) {
        throw new Error(`marker data returned ${response.status}`);
      }
      const categories = (await response.json()) as CategoryJson[];

      let tower = 0;
      let shrine = 0;
      let memory = 0;

      for (const category of categories) {
        const style = STYLES[category.name];
        if (!style) continue;

        const group = L.layerGroup();
        const icon = L.icon({
          iconUrl: `/map/botw/icons/${style.icon}.png`,
          iconSize: [style.width, style.height],
          iconAnchor: [style.width / 2, style.height],
          popupAnchor: [0, -style.height + 5],
        });

        for (const layer of category.layers ?? []) {
          for (const marker of layer.markers ?? []) {
            L.marker(marker.coords, { icon, title: marker.name })
              .bindPopup(
                `<strong>${escapeHtml(marker.name)}</strong>${
                  marker.tags?.includes('DLC')
                    ? '<br><span style="opacity:.65">DLC</span>'
                    : ''
                }`,
              )
              .addTo(group);
          }
        }

        groups[category.name] = group;
        const visible =
          (category.name === 'Sheikah Tower' && showTowers) ||
          (category.name === 'Shrine' && showShrines) ||
          (category.name === 'Memory' && showMemories);
        if (visible) group.addTo(instance);

        if (category.name === 'Sheikah Tower') tower = group.getLayers().length;
        if (category.name === 'Shrine') shrine = group.getLayers().length;
        if (category.name === 'Memory') memory = group.getLayers().length;
      }

      counts = { tower, shrine, memory };
    } catch (cause) {
      loadError = cause instanceof Error ? cause.message : String(cause);
    }
  }

  function syncLayers() {
    if (!map) return;
    setLayer('Sheikah Tower', showTowers);
    setLayer('Shrine', showShrines);
    setLayer('Memory', showMemories);
  }

  function setLayer(name: string, visible: boolean) {
    const group = groups[name];
    if (!map || !group) return;
    if (visible && !map.hasLayer(group)) group.addTo(map);
    if (!visible && map.hasLayer(group)) map.removeLayer(group);
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

    <div class="panel-divider"></div>

    <p class="panel-note">
      Completion state and Korok layers arrive once save ingestion lands. The map
      uses x/z; height is shown separately when live telemetry is connected.
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

    .map-status {
      bottom: 12px;
    }
  }
</style>

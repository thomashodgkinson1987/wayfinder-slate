<script lang="ts">
  import type { AppConfig } from '../lib/api';

  let { config }: { config: AppConfig | null } = $props();

  const modeName = $derived(
    config === null
      ? 'Connecting…'
      : config.mode === 'hub'
        ? 'Public hub'
        : config.mode === 'lan'
          ? 'LAN server'
          : 'Local',
  );

  const saveSource = $derived(
    config === null
      ? 'Checking…'
      : config.mode === 'hub'
        ? 'Upload or agent sync'
        : config.save_dir
          ? 'Folder detected'
          : 'Not configured',
  );

  const saveDone = $derived(config !== null && (config.mode === 'hub' || !!config.save_dir));
  const mapDone = $derived(config?.map_assets === true);
  const liveDone = $derived(config?.live === true);

  const stats = [
    { label: 'Shrines', hint: 'Awaiting first snapshot' },
    { label: 'Korok seeds', hint: 'Awaiting first snapshot' },
    { label: 'Sheikah towers', hint: 'Awaiting first snapshot' },
    { label: 'Recovered memories', hint: 'Awaiting first snapshot' },
  ];
</script>

<div class="page">
  <section class="hero">
    <div class="hero-main">
      <span class="eyebrow">Sheikah Slate companion</span>
      <h2>Your journey through Hyrule, remembered.</h2>
      <p>
        Wayfinder Slate reads your Breath of the Wild save and turns it into a live
        atlas: completion, play history, and — when Savage Wind is running — Link's
        position on the map. No Nintendo account, no console link, no save editing.
      </p>
    </div>

    <div class="hero-side">
      <div class="mini-card">
        <h3>Deployment</h3>
        <div class="mini-value">{modeName}</div>
        <div class="mini-note">
          {config?.mode === 'hub'
            ? 'Private saves by default'
            : 'Runs beside Savage Wind'}
        </div>
      </div>
      <div class="mini-card">
        <h3>Save source</h3>
        <div class="mini-value">{saveSource}</div>
        <div class="mini-note">
          {config?.save_dir ?? (config?.mode === 'hub' ? 'Uploads and agent sync' : 'Set WAYFINDER_SAVE_DIR')}
        </div>
      </div>
      <div class="mini-card">
        <h3>Live navigation</h3>
        <div class="mini-value">{liveDone ? 'Available' : 'Not connected'}</div>
        <div class="mini-note">
          {config?.mode === 'hub' ? 'Outbound agent push' : 'Savage Wind control socket'}
        </div>
      </div>
    </div>
  </section>

  <section class="grid" aria-label="Completion summary">
    {#each stats as stat (stat.label)}
      <article class="stat-card">
        <div class="stat-label">{stat.label}</div>
        <div class="stat-value">—</div>
        <div class="stat-hint">{stat.hint}</div>
      </article>
    {/each}
  </section>

  <div class="two-col">
    <section class="section">
      <div class="section-head">
        <div>
          <h2>First-run checklist</h2>
          <p>The pieces Wayfinder Slate needs before your save comes alive.</p>
        </div>
      </div>

      <div class="checklist">
        <div class="check" class:done={config !== null}>
          <span class="check-mark">{config !== null ? '✓' : '1'}</span>
          <div>
            <div class="check-title">Server connection</div>
            <div class="check-detail">
              {config !== null
                ? `Connected to Wayfinder Slate ${config.version}.`
                : 'Waiting for the Rust server on /api.'}
            </div>
          </div>
        </div>

        <div class="check" class:done={saveDone}>
          <span class="check-mark">{saveDone ? '✓' : '2'}</span>
          <div>
            <div class="check-title">Save folder</div>
            <div class="check-detail">
              {#if config?.mode === 'hub'}
                Uploads and agent sync arrive with the save-ingestion milestone.
              {:else if config?.save_dir}
                Found at <code>{config.save_dir}</code>
              {:else}
                Set <code class="inline-code">WAYFINDER_SAVE_DIR</code> to Savage
                Wind's <code class="inline-code">save</code> folder.
              {/if}
            </div>
          </div>
        </div>

        <div class="check" class:done={mapDone}>
          <span class="check-mark">{mapDone ? '✓' : '3'}</span>
          <div>
            <div class="check-title">Map assets</div>
            <div class="check-detail">
              {#if mapDone}
                BotW tiles and marker data are available.
              {:else}
                Run <code class="inline-code">./scripts/fetch-map-assets.sh</code>
                to download the attributed Zelda Dungeon tiles.
              {/if}
            </div>
          </div>
        </div>

        <div class="check" class:done={liveDone}>
          <span class="check-mark">{liveDone ? '✓' : '4'}</span>
          <div>
            <div class="check-title">Live telemetry</div>
            <div class="check-detail">
              {#if config?.mode === 'hub'}
                A local <code class="inline-code">wayfinder-agent</code> will push
                telemetry outbound; the hub never connects inward.
              {:else if config?.live}
                Start Savage Wind with
                <code class="inline-code">SAVAGEWIND_CONTROL=7777</code>.
                Direct polling lands in the live-mode milestone.
              {:else}
                Live mode is unavailable in this deployment.
              {/if}
            </div>
          </div>
        </div>
      </div>
    </section>

    <section class="section">
      <div class="section-head">
        <div>
          <h2>What happens next</h2>
          <p>The current build is the foundation, not the finished map.</p>
        </div>
      </div>

      <div class="timeline">
        <div class="timeline-item">
          <span class="timeline-phase">M2</span>
          <div>
            <div class="timeline-title">Save parser</div>
            <div class="timeline-desc">
              Typed keys, strings, vectors, inventory, and a 1.9.0 fixture test.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">M4</span>
          <div>
            <div class="timeline-title">Save ingestion</div>
            <div class="timeline-desc">
              Folder watching, debounced snapshots, uploads, and slot discovery.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">M5</span>
          <div>
            <div class="timeline-title">The map</div>
            <div class="timeline-desc">
              Shrine, tower, memory, and Korok layers with completion filters.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">M7</span>
          <div>
            <div class="timeline-title">Live Link</div>
            <div class="timeline-desc">
              Savage Wind polling locally, with an outbound agent for remote hubs.
            </div>
          </div>
        </div>
      </div>

      <div class="notice" style="margin-top: 16px">
        <strong>Early build.</strong>
        <span>
          The UI shell, parser crate, and control client are in place. Save-derived
          numbers will replace the dashes as ingestion lands.
        </span>
      </div>
    </section>
  </div>
</div>

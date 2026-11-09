<script lang="ts">
  import type { AppConfig } from '../lib/api';

  let { config }: { config: AppConfig | null } = $props();

  const modeName = $derived(
    config === null
      ? '—'
      : config.mode === 'hub'
        ? 'Public hub'
        : config.mode === 'lan'
          ? 'LAN server'
          : 'Local',
  );

  const platformSaveHint =
    'Linux: ~/.local/share/savagewind/save · Windows: set WAYFINDER_SAVE_DIR to the save folder beside savagewind.exe';
</script>

<div class="page">
  <section class="section" style="margin-bottom: 18px">
    <div class="section-head">
      <div>
        <span class="eyebrow">Runtime</span>
        <h2>Current configuration</h2>
        <p>Values reported by the running Wayfinder Slate server.</p>
      </div>
    </div>

    <dl class="kv">
      <dt>Deployment mode</dt>
      <dd>{modeName}</dd>
      <dt>Authentication</dt>
      <dd>{config?.auth_required ? 'Login required' : 'Not required (local)'}</dd>
      <dt>Server version</dt>
      <dd>{config?.version ?? '—'}</dd>
      <dt>Save folder</dt>
      <dd>{config?.save_dir ?? (config?.mode === 'hub' ? 'Uploads / agent sync' : 'Not configured')}</dd>
      <dt>Map assets</dt>
      <dd>{config?.map_assets ? 'Installed' : 'Not installed'}</dd>
      <dt>Live telemetry</dt>
      <dd>
        {config?.live
          ? `Direct Savage Wind polling (${config.control_addr ?? '127.0.0.1:7777'})`
          : config?.mode === 'hub'
            ? 'Outbound wayfinder-agent (planned)'
            : 'Unavailable'}
      </dd>
    </dl>
  </section>

  <div class="two-col">
    <section class="section">
      <div class="section-head">
        <div>
          <h2>Development setup</h2>
          <p>From a clean checkout to a running local server.</p>
        </div>
      </div>
      <pre class="command">./scripts/fetch-map-assets.sh
npm --prefix frontend install
npm --prefix frontend run build
cargo run -p wayfinder-server</pre>
      <p style="margin-top: 12px; color: var(--muted); font-size: 12px">
        For UI hot reload use <code class="inline-code">npm --prefix frontend run dev</code>;
        Vite proxies <code class="inline-code">/api</code> to the Rust server on port 8080.
      </p>
    </section>

    <section class="section">
      <div class="section-head">
        <div>
          <h2>Save discovery</h2>
          <p>Point the server at Savage Wind's save root.</p>
        </div>
      </div>
      <p style="color: var(--text-soft); font-size: 13px">
        {platformSaveHint}
      </p>
      <pre class="command">WAYFINDER_SAVE_DIR=/path/to/savagewind/save \
  cargo run -p wayfinder-server</pre>
      <p style="margin-top: 12px; color: var(--muted); font-size: 12px">
        The folder may contain <code class="inline-code">save_0/</code> with slots
        <code class="inline-code">0</code>–<code class="inline-code">5</code>, or point
        directly at a profile folder. Slots are discovered rather than hard-coded.
      </p>
    </section>
  </div>

  <section class="section" style="margin-top: 18px">
    <div class="section-head">
      <div>
        <h2>Live telemetry</h2>
        <p>Read-only data from Savage Wind's local control socket.</p>
      </div>
    </div>
    <pre class="command">SAVAGEWIND_CONTROL=7777 ./savagewind
cargo run -p wayfinder-agent -- probe</pre>
    <p style="margin-top: 12px; color: var(--text-soft); font-size: 13px">
      The control server binds to <code class="inline-code">127.0.0.1</code> only and
      is not HTTP. Wayfinder Slate uses only player-safe read commands:
      <code class="inline-code">pos</code>, <code class="inline-code">state</code>,
      <code class="inline-code">camera</code>, and
      <code class="inline-code">version</code>. On a remote hub, a local agent will
      push telemetry outbound; the hub never opens a connection to the player's PC.
    </p>
  </section>

  <div class="two-col" style="margin-top: 18px">
    <section class="section">
      <div class="section-head">
        <div>
          <h2>Deployment modes</h2>
          <p>One binary, three levels of exposure.</p>
        </div>
      </div>
      <div class="timeline">
        <div class="timeline-item">
          <span class="timeline-phase">Local</span>
          <div>
            <div class="timeline-title">On the gaming PC</div>
            <div class="timeline-desc">
              Loopback bind, saves watched directly, live polling enabled.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">LAN</span>
          <div>
            <div class="timeline-title">Second screen at home</div>
            <div class="timeline-desc">
              Bind to the LAN, require login, open the map on a phone or tablet.
            </div>
          </div>
        </div>
        <div class="timeline-item">
          <span class="timeline-phase">Hub</span>
          <div>
            <div class="timeline-title">Many players, one server</div>
            <div class="timeline-desc">
              Open registration, private saves by default, opt-in global statistics,
              outbound agent telemetry.
            </div>
          </div>
        </div>
      </div>
    </section>

    <section class="section">
      <div class="section-head">
        <div>
          <h2>Data &amp; attribution</h2>
          <p>What this project uses, and what it does not.</p>
        </div>
      </div>
      <p style="color: var(--text-soft); font-size: 13px">
        Map tiles, marker data, and icons come from
        <a href="https://github.com/zeldadungeon/maps">Zelda Dungeon's map repository</a>
        and remain the property of their creators. Wayfinder Slate uses the assets,
        not the upstream application code.
      </p>
      <p style="margin-top: 11px; color: var(--text-soft); font-size: 13px">
        Save-field names come from MIT-licensed community BotW research. Savage Wind
        is a separate fan project; Wayfinder Slate talks only to its documented,
        read-only control server.
      </p>
      <div class="notice" style="margin-top: 16px">
        <strong>Unofficial.</strong>
        <span>
          This is a fan companion. The Legend of Zelda and Breath of the Wild are
          trademarks of Nintendo. Not affiliated with or endorsed by Nintendo, Zelda
          Dungeon, or Savage Wind.
        </span>
      </div>
      <p style="margin-top: 14px; color: var(--muted); font-size: 11.5px">
        See <code class="inline-code">THIRD_PARTY_NOTICES.md</code> in the repository
        for the full notice.
      </p>
    </section>
  </div>
</div>

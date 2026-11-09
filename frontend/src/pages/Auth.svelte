<script lang="ts">
  import { login, register, type User } from '../lib/api';

  let {
    modeLabel,
    registrationOpen = true,
    onauthenticated,
  }: {
    modeLabel: string;
    registrationOpen?: boolean;
    onauthenticated: (user: User) => void;
  } = $props();

  type Tab = 'login' | 'register';
  let tab = $state<Tab>('login');
  let username = $state('');
  let password = $state('');
  let displayName = $state('');
  let error = $state<string | null>(null);
  let pending = $state(false);

  const isRegister = $derived(tab === 'register');

  function switchTab(next: Tab) {
    if (tab === next) return;
    tab = next;
    error = null;
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (pending) return;

    error = null;
    pending = true;
    try {
      const user = isRegister
        ? await register(username.trim(), password, displayName)
        : await login(username.trim(), password);
      onauthenticated(user);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      pending = false;
    }
  }
</script>

<div class="auth-shell">
  <div class="auth-card">
    <div class="auth-brand">
      <div class="brand-mark" aria-hidden="true">◭</div>
      <div>
        <h1 class="auth-title">Wayfinder Slate</h1>
        <p class="auth-subtitle">{modeLabel} · sign in to continue</p>
      </div>
    </div>

    <div class="auth-tabs" role="tablist" aria-label="Authentication">
      <button
        type="button"
        role="tab"
        class="auth-tab"
        class:active={!isRegister}
        aria-selected={!isRegister}
        onclick={() => switchTab('login')}
      >
        Sign in
      </button>
      {#if registrationOpen}
        <button
          type="button"
          role="tab"
          class="auth-tab"
          class:active={isRegister}
          aria-selected={isRegister}
          onclick={() => switchTab('register')}
        >
          Create account
        </button>
      {/if}
    </div>

    <form class="auth-form" onsubmit={submit}>
      <label class="field">
        <span class="field-label">Username</span>
        <input
          bind:value={username}
          autocomplete="username"
          autocapitalize="none"
          autocorrect="off"
          spellcheck="false"
          required
        />
      </label>

      {#if isRegister}
        <label class="field">
          <span class="field-label">Display name <em>(optional)</em></span>
          <input bind:value={displayName} autocomplete="nickname" maxlength="48" />
        </label>
      {/if}

      <label class="field">
        <span class="field-label">Password</span>
        <input
          type="password"
          bind:value={password}
          autocomplete={isRegister ? 'new-password' : 'current-password'}
          required
        />
      </label>

      {#if isRegister}
        <p class="auth-hint">
          Passwords are 8–128 characters. The first account registered becomes the
          administrator.
        </p>
      {/if}

      {#if error !== null}
        <p class="auth-error" role="alert">{error}</p>
      {/if}

      <button class="auth-submit" type="submit" disabled={pending}>
        {#if pending}
          Working…
        {:else if isRegister}
          Create account
        {:else}
          Sign in
        {/if}
      </button>
    </form>
  </div>
</div>

<style>
  .auth-shell {
    min-height: 100dvh;
    display: grid;
    place-items: center;
    padding: 24px;
  }

  .auth-card {
    width: min(420px, 100%);
    padding: 30px 28px 28px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: linear-gradient(150deg, rgba(15, 38, 48, 0.96), rgba(8, 23, 29, 0.96));
    box-shadow: var(--shadow);
    animation: auth-rise 220ms ease both;
  }

  @keyframes auth-rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .auth-brand {
    display: flex;
    align-items: center;
    gap: 13px;
    margin-bottom: 22px;
  }

  .brand-mark {
    width: 44px;
    height: 44px;
    flex: none;
    display: grid;
    place-items: center;
    border: 1px solid var(--line-bright);
    border-radius: 12px;
    background:
      linear-gradient(135deg, rgba(93, 219, 234, 0.16), transparent 60%),
      var(--panel-strong);
    color: var(--cyan);
    box-shadow: inset 0 0 22px rgba(93, 219, 234, 0.08);
    font-size: 20px;
  }

  .auth-title {
    font-size: 21px;
    line-height: 1.15;
  }

  .auth-subtitle {
    margin-top: 3px;
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .auth-tabs {
    display: flex;
    gap: 6px;
    margin-bottom: 20px;
    padding: 4px;
    border: 1px solid var(--line);
    border-radius: 11px;
    background: rgba(0, 0, 0, 0.2);
  }

  .auth-tab {
    flex: 1;
    padding: 8px 10px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--text-soft);
    font-size: 13px;
    cursor: pointer;
    transition:
      background 140ms ease,
      border-color 140ms ease,
      color 140ms ease;
  }

  .auth-tab:hover {
    color: var(--text);
  }

  .auth-tab.active {
    background: linear-gradient(90deg, rgba(93, 219, 234, 0.16), rgba(93, 219, 234, 0.04));
    border-color: rgba(93, 219, 234, 0.24);
    color: var(--text);
  }

  .auth-form {
    display: flex;
    flex-direction: column;
    gap: 15px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-label {
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.13em;
    text-transform: uppercase;
  }

  .field-label em {
    color: var(--muted);
    font-style: normal;
    letter-spacing: 0;
    text-transform: none;
  }

  .field input {
    padding: 10px 12px;
    border: 1px solid var(--line-bright);
    border-radius: var(--radius-sm);
    background: var(--panel-solid);
    color: var(--text);
    font-size: 14px;
  }

  .field input:focus {
    outline: none;
    border-color: var(--cyan-dim);
    box-shadow: 0 0 0 3px rgba(93, 219, 234, 0.14);
  }

  .auth-hint {
    color: var(--muted);
    font-size: 11.5px;
    line-height: 1.5;
  }

  .auth-error {
    padding: 9px 12px;
    border: 1px solid rgba(240, 139, 139, 0.4);
    border-radius: var(--radius-sm);
    background: rgba(50, 19, 22, 0.5);
    color: #ffd9d9;
    font-size: 12.5px;
  }

  .auth-submit {
    margin-top: 3px;
    padding: 11px 14px;
    border: 1px solid rgba(93, 219, 234, 0.35);
    border-radius: var(--radius-sm);
    background: linear-gradient(135deg, rgba(93, 219, 234, 0.22), rgba(93, 219, 234, 0.08));
    color: var(--text);
    font-size: 14px;
    cursor: pointer;
    transition:
      border-color 140ms ease,
      background 140ms ease,
      opacity 140ms ease;
  }

  .auth-submit:hover:not(:disabled) {
    border-color: rgba(93, 219, 234, 0.55);
    background: linear-gradient(135deg, rgba(93, 219, 234, 0.3), rgba(93, 219, 234, 0.12));
  }

  .auth-submit:disabled {
    opacity: 0.6;
    cursor: wait;
  }
</style>

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke, isTauri } from '@tauri-apps/api/core';
import './styles.css';

type Mode = 'limit' | 'top_up' | 'calibration';
type Config = { schema: number; limit: number; band: number; mode: Mode; heat_c: number | null; discharge: boolean; notifications: boolean };
type Battery = { percent: number; state: string; power_w: number | null; temperature_c: number | null; cycles: number | null; health: number | null };
type Status = {
  config: Config; snapshot: { batteries: Battery[]; ac: boolean | null; fresh: boolean };
  capabilities: { set_limit: boolean; thresholds: boolean; inhibit: boolean; force_discharge: boolean; temperature: boolean };
  simulated: boolean; mechanism: string; reason: string; phase: string;
  discovery: { manufacturer: string; model: string; bios: string; wmi: boolean; cctk: boolean; note: string };
  config_error: string | null;
};
const desktop = isTauri();
const defaults: Config = { schema: 1, limit: 80, band: 5, mode: 'limit', heat_c: null, discharge: false, notifications: true };
const preview: Status = {
  config: defaults, snapshot: { batteries: [], ac: null, fresh: false },
  capabilities: { set_limit: false, thresholds: false, inhibit: false, force_discharge: false, temperature: false },
  simulated: false, mechanism: 'Browser preview · no device connection', reason: 'Open the Windows desktop app for battery monitoring and simulation.', phase: 'Limit',
  discovery: { manufacturer: '', model: '', bios: '', wmi: false, cctk: false, note: 'Hardware discovery runs locally in the Windows app.' }, config_error: null,
};
const number = (value: number | null | undefined, suffix = '', digits = 0) => value == null || !Number.isFinite(value) ? '—' : `${value.toFixed(digits)}${suffix}`;

function App() {
  const [status, setStatus] = useState<Status>(preview);
  const [draft, setDraft] = useState<Config>(defaults);
  const [dirty, setDirty] = useState(false);
  const dirtyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [login, setLogin] = useState(false);
  const [selectedBattery, setSelectedBattery] = useState(0);
  const [simPercent, setSimPercent] = useState(68);
  const [simTemp, setSimTemp] = useState(31);
  const [simAc, setSimAc] = useState(true);
  const refreshing = useRef(false);

  const refresh = useCallback(async () => {
    if (!desktop || refreshing.current) return;
    refreshing.current = true;
    try {
      const next = await invoke<Status>('get_status');
      setStatus(next);
      if (!dirtyRef.current) setDraft(next.config);
      setError('');
    } catch (e) { setError(String(e)); }
    finally { refreshing.current = false; }
  }, []);

  useEffect(() => {
    void refresh();
    if (!desktop) return;
    const timer = window.setInterval(() => { void refresh(); }, 2000);
    void invoke<boolean>('get_launch_at_login').then(setLogin).catch((e) => setError(String(e)));
    return () => window.clearInterval(timer);
  }, [refresh]);

  const edit = (value: Partial<Config>) => {
    dirtyRef.current = true; setDirty(true); setDraft((current) => ({ ...current, ...value })); setMessage('');
  };
  const action = async (run: () => Promise<unknown>, success = '') => {
    if (busy || !desktop) return;
    setBusy(true); setError(''); setMessage('');
    try { await run(); if (success) setMessage(success); await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const save = () => action(async () => {
    await invoke('save_config', { config: draft }); dirtyRef.current = false; setDirty(false);
  }, status.simulated ? 'Simulation settings applied. Your BIOS is unchanged.' : 'Reminder settings saved. Charging is controlled by your firmware.');
  const toggleSimulation = () => action(async () => {
    await invoke('set_simulation', { enabled: !status.simulated });
    dirtyRef.current = false; setDirty(false);
  });

  const batteries = status.snapshot.batteries;
  const battery = batteries[Math.min(selectedBattery, Math.max(0, batteries.length - 1))];
  const caps = status.capabilities;
  const connected = desktop && !busy;
  const controls = connected && !status.config_error;
  const enforced = status.simulated && caps.set_limit;
  const phaseNames: Record<string, string> = { TopUp: 'Charging to 100%', FullHold: 'Holding full · 10 min', Discharging: 'Discharging to 25%', Recharging: 'Recharging to full', Complete: 'Complete · returning to limit', Limit: 'Limit' };
  const stateLabel = battery ? battery.state === 'not charging' ? 'Not charging' : battery.state.charAt(0).toUpperCase() + battery.state.slice(1) : desktop ? 'No battery detected' : 'Awaiting desktop connection';
  const canTravel = caps.set_limit || caps.inhibit;
  const canCalibrate = caps.inhibit && caps.force_discharge;
  const canHeat = caps.temperature && caps.inhibit;
  const progress = battery?.percent ?? 0;

  return <main className="app-shell">
    <header className="app-header">
      <div className="brand"><span className="brand-mark" aria-hidden="true"><svg width="23" height="23" viewBox="0 0 24 24" fill="none"><path d="M13 2 5 14h6l-1 8 9-13h-6l1-7Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round"/></svg></span><div><h1>Chargekeeper<span className="build-label">DEV</span></h1><p>A little less charge. A longer life.</p></div></div>
      <span className="privacy"><span aria-hidden="true">◈</span> LOCAL & OFFLINE</span>
    </header>

    {!desktop && <div className="notice"><strong>Browser preview</strong> · No live battery data or hardware controls. Run the Windows app to connect your device.</div>}
    {status.simulated && <div className="notice simulation-notice"><strong>Simulation is on.</strong> All battery values and controls below are simulated. Your hardware is unchanged.</div>}
    {desktop && !status.snapshot.fresh && <div className="notice" role="status"><strong>Battery readings unavailable.</strong> {status.reason}</div>}
    {error && <div className="notice error" role="alert">{error}<span>Displayed readings may be out of date.</span></div>}
    {status.config_error && <div className="notice error" role="alert">Configuration needs attention: {status.config_error}. The existing file has been preserved.</div>}

    <section className="hero card" aria-labelledby="battery-heading">
      <div className="hero-copy"><span className="eyebrow" id="battery-heading">BATTERY STATUS</span>
        <div className="charge-number">{number(battery?.percent)}<span>%</span></div>
        <div className="status-line"><span className={`status-dot ${battery?.state === 'charging' ? 'active' : ''}`} aria-hidden="true"/>{stateLabel}</div>
        <p className="power-source">{status.snapshot.ac === true ? 'Connected to AC power' : status.snapshot.ac === false ? 'Running on battery' : 'Power source unavailable'}</p>
      </div>
      <div className="battery-scene" aria-hidden="true"><div className="battery-orbit"/><div className="battery-art"><div className="battery-cap"/><div className="battery-case"><div className="battery-fill" style={{ height: `${Math.max(0, Math.min(100, progress))}%` }}/><svg className="battery-bolt" width="43" height="55" viewBox="0 0 36 48" fill="none"><path d="M21 2 3 28h13l-2 18 19-28H20L21 2Z" fill="currentColor"/></svg><div className="battery-shine"/></div><div className="battery-base"/></div><div className="orbit-caption">{status.simulated ? 'SIMULATED CELL' : 'BATTERY CARE, SIMPLIFIED'}</div></div>
      {batteries.length > 1 && <label className="battery-select">Battery <select value={Math.min(selectedBattery, batteries.length - 1)} onChange={(e) => setSelectedBattery(Number(e.target.value))}>{batteries.map((_, i) => <option key={i} value={i}>Pack {i + 1}</option>)}</select></label>}
      <div className="metrics"><Metric label="Battery power" value={number(battery?.power_w, ' W', 1)}/><Metric label="Temperature" value={number(battery?.temperature_c, ' °C', 1)}/><Metric label="Cycle count" value={number(battery?.cycles)}/><Metric label="Battery health" value={number(battery?.health, '%', 1)}/></div>
    </section>

    <section className="mechanism" aria-label="Charging mechanism"><span className="mechanism-icon" aria-hidden="true">◇</span><div><strong>{enforced ? 'Simulated firmware control' : 'Software reminders only'}</strong><p>{status.mechanism}</p></div><span className="mechanism-tag">{status.simulated ? 'SIMULATION' : 'READ ONLY'}</span></section>

    <section className="card settings" aria-labelledby="limit-heading">
      <div className="section-heading"><div><span className="eyebrow">YOUR DAILY BALANCE</span><h2 id="limit-heading">{enforced ? 'Charge limit' : 'Charge reminder'}</h2></div><div className="limit-value">{draft.limit}<span>%</span></div></div>
      <label className="sr-only" htmlFor="limit">{enforced ? 'Maximum charge limit' : 'Unplug reminder threshold'}</label>
      <input id="limit" className="charge-range" type="range" min="50" max="100" step="1" value={draft.limit} disabled={!controls} onChange={(e) => edit({ limit: Number(e.target.value) })} style={{ '--range': `${(draft.limit - 50) * 2}%` } as React.CSSProperties}/>
      <div className="range-labels"><span>50%</span><span className="recommended">80% · DAILY SWEET SPOT</span><span>100%</span></div>
      <p className="hint">{enforced ? `The simulated battery resumes charging below ${draft.limit - draft.band}% and stops at ${draft.limit}%.` : `Get an unplug reminder at ${draft.limit}%. This app cannot stop charging on your device.`}</p>

      <div className="mode-grid" role="group" aria-label="Charging mode">
        <ModeButton title="Daily limit" description={enforced ? 'Stay in your preferred range' : 'A gentle unplug reminder'} glyph="◎" selected={draft.mode === 'limit'} disabled={!controls} onClick={() => edit({ mode: 'limit' })}/>
        <ModeButton title="Top up once" description={canTravel ? '100%, then back to your limit' : 'Requires charge control'} glyph="↗" selected={draft.mode === 'top_up'} disabled={!controls || !canTravel} onClick={() => edit({ mode: 'top_up' })}/>
        <ModeButton title="Calibration" description={canCalibrate ? 'Charge, hold, discharge, recharge' : 'Requires discharge control'} glyph="↻" selected={draft.mode === 'calibration'} disabled={!controls || !canCalibrate} onClick={() => edit({ mode: 'calibration' })}/>
      </div>
      {status.simulated && status.config.mode !== 'limit' && <div className="phase" role="status"><span className="status-dot active"/>{phaseNames[status.phase] ?? status.phase}</div>}

      <div className="settings-rows">
        <div className={`setting-row ${!caps.thresholds ? 'unavailable' : ''}`}><div><label htmlFor="band">Sailing range</label><p>{caps.thresholds ? 'Let the battery drift before charging again' : 'Requires hardware start / stop thresholds'}</p></div><div className="number-control"><input id="band" type="number" min={caps.set_limit ? 5 : 2} max="20" value={draft.band} disabled={!controls || !caps.thresholds} onChange={(e) => edit({ band: Number(e.target.value) })}/><span>%</span></div></div>
        <div className={`setting-row ${!canHeat ? 'unavailable' : ''}`}><div><label htmlFor="heat">Heat protection</label><p>{canHeat ? 'Pause charging above the selected temperature' : 'Requires temperature data and charge inhibition'}</p></div><div className="row-actions">{draft.heat_c !== null && <label className="temperature-control"><span className="sr-only">Heat threshold in Celsius</span><input type="number" min="35" max="50" value={draft.heat_c} disabled={!controls || !canHeat} onChange={(e) => edit({ heat_c: Number(e.target.value) })}/>°C</label>}<input id="heat" className="switch" type="checkbox" checked={draft.heat_c !== null} disabled={!controls || !canHeat} onChange={(e) => edit({ heat_c: e.target.checked ? 42 : null })}/></div></div>
        <ToggleRow id="discharge" title="Discharge to limit" description={caps.force_discharge ? 'Use battery power when above your cap on AC' : 'Unavailable: this device cannot force discharge'} checked={draft.discharge} disabled={!controls || !caps.force_discharge} onChange={(value) => edit({ discharge: value })}/>
      </div>
      <div className="apply-row"><p>{dirty ? 'You have unapplied changes.' : 'A quieter routine for your battery.'}</p><button className="primary" disabled={!controls || !dirty || draft.band < (caps.set_limit ? 5 : 2) || draft.band > 20} onClick={() => { void save(); }}>{busy ? 'Working…' : 'Apply settings'}<span aria-hidden="true">↗</span></button></div>
    </section>

    <details className="card details"><summary>Preferences & device details<span aria-hidden="true">＋</span></summary><div className="details-body">
      <ToggleRow id="notifications" title="Threshold notifications" description="A local reminder when it is time to unplug" checked={draft.notifications} disabled={!controls} onChange={(value) => edit({ notifications: value })}/>
      <ToggleRow id="login" title="Launch at login" description="Start in your Windows user session" checked={login} disabled={!connected} onChange={(value) => { void action(async () => { await invoke('set_launch_at_login', { enabled: value }); setLogin(value); }); }}/>
      <div className="device-info"><span className="eyebrow">CONNECTED DEVICE</span><strong>{status.discovery.model || 'Device not identified'}</strong><p>{status.discovery.manufacturer || (desktop ? 'Hardware discovery unavailable' : 'Windows desktop app required')}{status.discovery.bios ? ` · BIOS ${status.discovery.bios}` : ''}</p><p>{status.discovery.note}</p><p className="hint">Missing readings are shown as —. No battery temperature or health values are estimated.</p>
        {desktop && /^dell\b/i.test(status.discovery.manufacturer) && !status.discovery.wmi && !status.discovery.cctk && <p>For Dell hardware support, install <button className="text-link" disabled={!connected} onClick={() => { void action(() => invoke('open_dell_support')); }}>Dell Command | Configure</button>. This development build still requires a validated controller before firmware changes.</p>}
      </div>
      <div className="utility-actions"><button className="secondary" disabled={!connected} onClick={() => { void action(async () => { const path = await invoke<string>('export_diagnostics'); setMessage(`Diagnostics saved to ${path}`); }); }}>Export diagnostics</button><button className="secondary" disabled={!connected || !status.simulated} title={status.simulated ? 'Reset simulated charging to 100%' : 'No validated BIOS writer is installed'} onClick={() => { void action(async () => { await invoke('restore_defaults'); dirtyRef.current = false; setDirty(false); }, 'Simulated Standard charging restored. Your BIOS is unchanged.'); }}>Restore Standard charging</button></div>
    </div></details>

    <details className="card details simulator"><summary>Battery simulator<span className="detail-badge">DEVELOPMENT</span><span aria-hidden="true">＋</span></summary><div className="details-body"><ToggleRow id="simulation" title="Simulate a connected battery" description="Explore every mode without changing your hardware" checked={status.simulated} disabled={!connected} onChange={() => { void toggleSimulation(); }}/>{status.simulated && <div className="simulation-controls"><label>Battery %<input type="number" min="0" max="100" value={simPercent} onChange={(e) => setSimPercent(Number(e.target.value))}/></label><label>Temperature °C<input type="number" min="0" max="60" value={simTemp} onChange={(e) => setSimTemp(Number(e.target.value))}/></label><label className="checkbox-label"><input type="checkbox" checked={simAc} onChange={(e) => setSimAc(e.target.checked)}/>AC connected</label><button className="secondary" disabled={!connected || simPercent < 0 || simPercent > 100 || simTemp < 0 || simTemp > 60} onClick={() => { void action(() => invoke('set_simulated_battery', { percent: simPercent, temperature: simTemp, ac: simAc })); }}>Set sample</button><p className="hint">Simulation uses an accelerated clock. Values are not readings from your battery.</p></div>}</div></details>

    <div className="feedback" role="status" aria-live="polite">{message}</div>
    <footer><span className="footer-mark" aria-hidden="true">✦</span><span>Designed to stay out of your way.</span><span className="footer-version">v0.1.0 · Development</span></footer>
  </main>;
}

function Metric({ label, value }: { label: string; value: string }) { return <div className="metric"><span>{label}</span><strong>{value}</strong></div>; }
function ModeButton({ title, description, glyph, selected, disabled, onClick }: { title: string; description: string; glyph: string; selected: boolean; disabled: boolean; onClick: () => void }) {
  return <button className={`mode-button ${selected ? 'selected' : ''}`} aria-pressed={selected} disabled={disabled} onClick={onClick}><span className="mode-glyph" aria-hidden="true">{glyph}</span><strong>{title}</strong><span>{description}</span>{selected && <span className="selected-dot" aria-hidden="true"/>}</button>;
}
function ToggleRow({ id, title, description, checked, disabled, onChange }: { id: string; title: string; description: string; checked: boolean; disabled: boolean; onChange: (value: boolean) => void }) {
  return <div className={`setting-row ${disabled ? 'unavailable' : ''}`}><div><label htmlFor={id}>{title}</label><p>{description}</p></div><input id={id} className="switch" type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)}/></div>;
}

createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);

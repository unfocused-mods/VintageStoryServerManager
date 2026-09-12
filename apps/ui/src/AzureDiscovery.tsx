import { useEffect, useRef, useState } from "react";
import { type Account, type Api, type Server } from "./api";

export interface DiscoveredDeployment {
  server: Server;
  vm_id: string;
  resource_group: string;
  already_managed: boolean;
  warnings: string[];
}
interface DiscoveryResult { deployments: DiscoveredDeployment[]; warnings: string[] }

export default function AzureDiscovery({ api, account, onAdded, onClose }: {
  api: Api; account: Account; onAdded?: () => void; onClose: () => void;
}) {
  const [result, setResult] = useState<DiscoveryResult | null>(null);
  const [selected, setSelected] = useState("");
  const [keyPath, setKeyPath] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [recoveryWarnings, setRecoveryWarnings] = useState<string[]>([]);
  const generation = useRef(0);
  const adopting = useRef(false);
  async function discover() {
    const current = ++generation.current;
    setBusy(true); setError(""); setNotice(""); setRecoveryWarnings([]); setSelected(""); setResult(null);
    try {
      const found = await api.request<DiscoveryResult>(`/accounts/${account.id}/discover`);
      if (current === generation.current) setResult(found);
    } catch (e) { if (current === generation.current) setError(String(e)); }
    finally { if (current === generation.current) setBusy(false); }
  }
  useEffect(() => { void discover(); return () => { generation.current++; }; }, [api, account.id]);
  const candidate = result?.deployments.find((item) => item.vm_id === selected);
  async function adopt() {
    if (!candidate || adopting.current) return;
    adopting.current = true;
    const current = generation.current;
    setBusy(true); setError(""); setNotice(""); setRecoveryWarnings([]);
    try {
      const server = await api.request<Server & { recovery_warnings?: string[] }>(`/accounts/${account.id}/adopt`, "POST", {
        vm_id: candidate.vm_id,
        name: name.trim() || candidate.server.name,
        ...(keyPath.trim() ? { ssh_private_key: keyPath.trim() } : {}),
      });
      if (current !== generation.current) return;
      setResult((value) => value && ({ ...value, deployments: value.deployments.map((item) =>
        item.vm_id === candidate.vm_id ? { ...item, server, already_managed: true } : item) }));
      setSelected(""); setKeyPath("");
      setNotice(`${server.name} added. Open Infrastructure to reconnect to its Remote.`);
      setRecoveryWarnings(server.recovery_warnings || []);
      onAdded?.();
    } catch (e) { if (current === generation.current) setError(String(e)); }
    finally { adopting.current = false; if (current === generation.current) setBusy(false); }
  }
  return <section className="detail-panel form" aria-label="Discover Azure servers">
    <h3>Find existing servers</h3>
    <p className="muted small">Search {account.name} for deployments created by Server Manager. Adding a server keeps its existing game files and settings.</p>
    {busy && !result && <p role="status">Searching Azure…</p>}
    {result?.warnings.map((warning, i) => <p className="muted small" key={i}>{warning}</p>)}
    {result && !result.deployments.length && <p>No Server Manager deployments found in this subscription.</p>}
    {result?.deployments.map((item) => <label className="discovery-option" key={item.vm_id}>
      <input type="radio" name="discovered-server" checked={selected === item.vm_id}
        disabled={busy || item.already_managed} onChange={() => { setSelected(item.vm_id); setName(item.server.name); setKeyPath(""); setError(""); }} />
      <span><strong>{item.server.name}</strong><span className="muted small">{item.server.region} · {item.server.size}</span>
        <span className="muted small">{item.resource_group}</span>
        {item.already_managed && <span className="small">Already in this Manager</span>}</span>
    </label>)}
    {candidate && <div className="form">
      <p className="muted small">Stop managing this server from the old Manager before reconnecting here.</p>
      <label className="field">Server name<input value={name} disabled={busy} onChange={(e) => setName(e.target.value)} /></label>
      {candidate.warnings.map((warning, i) => <p className="muted small" key={i}>{warning}</p>)}
      <label className="field">SSH private key path (optional)
        <input value={keyPath} disabled={busy} onChange={(e) => setKeyPath(e.target.value)} placeholder="Path to your existing private key" />
      </label>
      <p className="muted small">Without the existing SSH key, you can view VM status and start the VM. Game controls need that key on the computer running the Manager and SSH access allowed from its address.</p>
      <button type="button" className="button primary" disabled={busy} onClick={() => void adopt()}>{busy ? "Adding…" : "Add to Manager"}</button>
    </div>}
    {notice && <p role="status">{notice}</p>}
    {recoveryWarnings.length > 0 && <div className="form" role="status" aria-label="Recovery notes">
      {recoveryWarnings.map((warning, i) => <p className="muted small" key={i}>{warning}</p>)}
    </div>}
    {error && <p role="alert" className="inline-error">{error}</p>}
    <div className="button-row">
      <button type="button" className="button secondary" disabled={busy} onClick={() => void discover()}>Search again</button>
      <button type="button" className="button secondary" disabled={busy} onClick={onClose}>Close discovery</button>
    </div>
  </section>;
}

import {useState} from "react";
import {api} from "../api";
export function GithubSettings(){
 const [state,setState]=useState<Awaited<ReturnType<typeof api.githubSettings>>|null>(null);
 const [app,setApp]=useState("");const [installation,setInstallation]=useState("");const [repo,setRepo]=useState("");const [checkout,setCheckout]=useState("");const [key,setKey]=useState("");const [login,setLogin]=useState("");const [message,setMessage]=useState("");const [busy,setBusy]=useState(false);
 const load=async()=>{try{setState(await api.githubSettings())}catch(e){setMessage((e as Error).message)}};
 const run=async(action:()=>Promise<unknown>)=>{setBusy(true);setMessage("");try{await action();await load()}catch(e){setMessage((e as Error).message)}finally{setKey("");setBusy(false)}};
 return <details className="disclosure settings-group form-stack" onToggle={e=>{if(e.currentTarget.open&&!state)void load()}}><summary>GitHub App setup</summary>
 <ol><li><a href="https://github.com/settings/apps/new" target="_blank" rel="noreferrer">Create a GitHub App</a> with Issues, Pull requests, Contents and Checks write; Commit statuses, Actions, Administration and Metadata read. Disable webhooks. Generate an RSA private key, then install the App on the selected repository.</li><li>Connect the installation below. This browser is already protected by your passkey. The private key is stored in macOS Keychain and is never given to agents.</li><li>Review the numeric approver list, then use Engineering OS Repair and Verify model routes below. Review provider and never-sent file policies before enabling work.</li></ol>
 <p>Review OpenRouter’s provider training exclusions and ChatGPT’s “Improve the model for everyone” setting.</p>
 <label>App ID<input value={app} onChange={e=>setApp(e.target.value)} inputMode="numeric"/></label>
 <label>Installation ID<input value={installation} onChange={e=>setInstallation(e.target.value)} inputMode="numeric"/></label>
 <label>Repository (owner/name)<input value={repo} onChange={e=>setRepo(e.target.value)}/></label>
 <label>Local checkout (absolute path)<input value={checkout} onChange={e=>setCheckout(e.target.value)}/></label>
 <label>RSA private key (PEM; leave empty to reuse Keychain)<textarea autoComplete="off" value={key} onChange={e=>setKey(e.target.value)} rows={4}/></label>
 <button className="btn" disabled={busy||!app||!installation||!repo||!checkout} onClick={()=>void run(async()=>{const result=await api.configureGithub({app_id:Number(app),installation_id:Number(installation),repo,checkout,private_key:key||null});setMessage(result.preflight.warnings.length?result.preflight.warnings.join(". "):"Preflight passed. Review allowlist, worker and model routes next.")})}>Connect with passkey and run preflight</button>
 {state?.repositories.map(r=><p key={r.project}>{r.full_name} — repository ID {r.repo_id}. <a href={`https://github.com/${r.full_name}/settings/branches`} target="_blank" rel="noreferrer">Review branch protection</a>. Add plantool/policy as a required check after the PR workflow can publish it.</p>)}
 <p>Initial approver: amritghimire. Usernames are resolved through GitHub; decisions use numeric IDs, not names. Changes require Touch ID and are audited.</p>
 <ul>{state?.allowlist.map(u=><li key={u.id}>{u.login} ({u.id})</li>)}</ul>
 <label>GitHub username<input value={login} onChange={e=>setLogin(e.target.value)}/></label>
 <button className="btn" disabled={busy||!login} onClick={()=>void run(()=>api.githubAllowlist(login,false))}>Add approver with passkey</button>
 <button className="btn" disabled={busy||!login} onClick={()=>void run(()=>api.githubAllowlist(login,true))}>Remove approver with passkey</button>
 {state?.sync.map(s=><p key={s.project}>Last sync: {s.synced_at??"not yet"}{s.error&&` — ${s.error}`}</p>)}
 {message&&<p role="status">{message}</p>}</details>
}

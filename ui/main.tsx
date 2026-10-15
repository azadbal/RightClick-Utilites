import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { open } from '@tauri-apps/plugin-dialog';
import './style.css';

type FunctionEntry = { id: string; label: string; enabled: boolean; appliesTo: 'file'|'folder'|'any'; extensions: string[]; executable: string; args: string[]; supportsMultiple?: boolean };
type Config = { version: number; functions: FunctionEntry[] };
type Job = { running: boolean; progress: number|null; message: string; logs: string[]; result: {success:boolean; output?:string; error?:string; bytes?:number; outputs?:{input:string;output:string;bytes:number;width:number;height:number}[]; errors?:{input:string;error:string}[]}|null };
const emptyJob: Job = {running:false,progress:null,message:'',logs:[],result:null};

function App() {
  const [config,setConfig] = useState<Config>({version:1,functions:[]});
  const [selected,setSelected] = useState('video-20mb');
  const [inputs,setInputs] = useState<string[]>([]);
  const [job,setJob] = useState<Job>(emptyJob);
  const [error,setError] = useState('');
  const [editor,setEditor] = useState<FunctionEntry|null>(null);
  const [editorExtensions,setEditorExtensions] = useState('');
  const [isNew,setIsNew] = useState(false);
  const [saving,setSaving] = useState(false);
  const [starting,setStarting] = useState(false);
  const [dragging,setDragging] = useState(false);
  const current = config.functions.find(f=>f.id===selected);
  const busy = job.running || starting || saving;

  const run = async (id=selected,paths=inputs) => {
    setError(''); setStarting(true); setJob(emptyJob);
    try { await invoke('start_job',{functionId:id,inputs:paths}); setJob(await invoke<Job>('job_status')); }
    catch(e) {setError(String(e));} finally {setStarting(false);}
  };
  useEffect(()=>{
    let mounted=true;
    (async()=>{
      try {
        const loaded=await invoke<Config>('get_config');
        const context=await invoke<{functionId:string|null;inputs:string[]}>('launch_context');
        if(!mounted) return;
        setConfig(loaded);
        if(context.inputs.length) setInputs(context.inputs);
        if(context.functionId) {setSelected(context.functionId); if(context.inputs.length) await run(context.functionId,context.inputs);}
      } catch(e){if(mounted)setError(String(e));}
    })();
    const timer=setInterval(()=>invoke<Job>('job_status').then(j=>{if(mounted)setJob(j);}).catch(()=>{}),400);
    const unlisten=getCurrentWebviewWindow().onDragDropEvent(({payload})=>{
      setDragging(payload.type==='over' || payload.type==='enter');
      if(payload.type==='drop') {
        setInputs(payload.paths);setError('');
      }
    });
    return()=>{mounted=false;clearInterval(timer);unlisten.then(fn=>fn());};
  },[]);

  const persist = async (next:Config) => {
    setSaving(true); setError('');
    try {await invoke('save_config',{config:next});setConfig(next);return true;}
    catch(e){setError(String(e));return false;} finally{setSaving(false);}
  };
  const browse = async (directory=false) => {
    try {const path=await open({directory,multiple:!directory&&!!current?.supportsMultiple,title:directory?'Choose a folder':'Choose files'});if(path){setInputs(typeof path==='string'?[path]:path);setError('');}}
    catch(e){setError(String(e));}
  };
  const reorder = async (direction:number) => {
    const items=[...config.functions],index=items.findIndex(f=>f.id===selected),next=index+direction;
    if(next<0 || next>=items.length)return;
    [items[index],items[next]]=[items[next],items[index]];
    await persist({...config,functions:items});
  };
  const edit = (entry?:FunctionEntry) => {
    setEditorExtensions(entry?.extensions.join(', ')||'');
    setIsNew(!entry);setEditor(entry?{...entry,args:[...entry.args],extensions:[...entry.extensions]}:{id:`function-${Date.now()}`,label:'',enabled:true,appliesTo:'file',extensions:[],executable:'',args:['{input}']});setError('');
  };
  const saveEditor = async () => {
    if(!editor)return;
    const next={...editor,label:editor.label.trim(),executable:editor.executable.trim(),extensions:editorExtensions.split(',').map(x=>x.trim().toLowerCase()).filter(Boolean)};
    const functions=isNew?[...config.functions,next]:config.functions.map(f=>f.id===editor.id?next:f);
    if(await persist({...config,functions})){setSelected(next.id);setEditor(null);}
  };

  return <main>
    <header><div className="brand"><span className="brand-icon">↳</span><div><h1>RightClick Utilities</h1><p>Your shortcuts, one right-click away.</p></div></div><span className="tag">WINDOWS 11</span></header>
    <div className="layout">
      <section className="functions panel"><div className="section-title"><h2>Your functions</h2><span>{config.functions.length}</span></div>
        <p className="hint">Enabled functions appear in the RightClick Utilities menu when they match your selection.</p>
        <div className="function-list">{config.functions.map(f=><button key={f.id} className={`function ${selected===f.id?'selected':''}`} onClick={()=>{setSelected(f.id);setError('');}} disabled={busy}>
          <span className="function-icon">{f.id==='strip-audio'?'♫':'↳'}</span><span className="function-copy"><strong>{f.label}</strong><small>{f.appliesTo==='folder'?'Folders':f.extensions.length?f.extensions.join(' · '):f.appliesTo==='any'?'Files & folders':'All files'}</small></span><span className={`dot ${f.enabled?'enabled':''}`} title={f.enabled?'Enabled':'Disabled'}/>
        </button>)}</div>
        <button className="add" onClick={()=>edit()} disabled={busy}>＋ Add function</button>
        <div className="list-actions"><button onClick={()=>reorder(-1)} disabled={busy||!current||config.functions[0]?.id===selected}>↑ Move up</button><button onClick={()=>reorder(1)} disabled={busy||!current||config.functions.at(-1)?.id===selected}>↓ Move down</button></div>
      </section>
      <section className="runner panel"><div className="section-title"><h2>{current?.label||'Choose a function'}</h2><button onClick={()=>edit(current)} disabled={busy||!current}>Edit</button></div>
        {current&&<label className="enable"><input type="checkbox" checked={current.enabled} disabled={busy} onChange={e=>persist({...config,functions:config.functions.map(f=>f.id===selected?{...f,enabled:e.target.checked}:f)})}/> Show in right-click menu</label>}
        <div className={`drop-zone ${dragging?'dragging':''}`}><span className="drop-icon">▱</span><strong>Drop files or a folder here</strong><span>Or choose an input below</span></div>
        <label className="field">{inputs.length>1?`Inputs (${inputs.length})`:'Input'}<textarea rows={Math.min(Math.max(inputs.length,1),4)} value={inputs.join('\n')} onChange={e=>setInputs(e.target.value.split('\n').filter(Boolean))} placeholder="Choose files or a folder…" disabled={busy}/></label>
        <div className="browse-actions"><button onClick={()=>browse()} disabled={busy}>Choose files…</button><button onClick={()=>browse(true)} disabled={busy}>Choose folder…</button></div>
        <p className="output-hint">{selected==='png-jpeg'?'JPEG quality 90%. Transparent areas become white. Folders include PNGs directly inside them.':'Results are saved beside the original with a new name.'}</p>
        {inputs.length>1&&!current?.supportsMultiple&&<p className="hint">This function accepts one file or folder at a time.</p>}
        <button className="primary run" onClick={()=>run()} disabled={busy||!current?.enabled||!inputs.length||(inputs.length>1&&!current?.supportsMultiple)}>{busy?'Working…':'Run function'} <span>→</span></button>
        {(job.running||job.result)&&<div className={`job ${job.result&&!job.result.success?'failed':''}`} aria-live="polite">
          <div className="job-title"><strong>{job.running?'Processing':job.result?.success?'Complete':'Stopped'}</strong>{job.running&&<button onClick={()=>invoke('cancel_job').catch(e=>setError(String(e)))}>Cancel</button>}</div>
          <p>{job.message}</p>{job.running&&<progress max="100" value={job.progress??undefined}/>}
          {job.result?.output&&<><div className="output-path">{job.result.output}</div><button onClick={()=>invoke('open_output',{path:job.result!.output}).catch(e=>setError(String(e)))}>Open output</button></>}
          {job.result?.outputs?.map(item=><div key={item.output}><div className="output-path">{item.output}</div><button onClick={()=>invoke('open_output',{path:item.output}).catch(e=>setError(String(e)))}>Open output</button></div>)}
          {job.result?.errors?.map((failure,i)=><p key={i} className="error">{failure.input}: {failure.error}</p>)}
          {job.logs.length>0&&<details><summary>Processing log</summary><pre>{job.logs.join('\n')}</pre></details>}
        </div>}
      </section>
    </div>
    {error&&<div className="error" role="alert">{error}</div>}
    <footer>Functions are saved automatically. PNG → JPEG also supports multiple files.<span>v0.1.0</span></footer>
    {editor&&<div className="modal-backdrop"><section className="editor panel" role="dialog" aria-modal="true" aria-label={isNew?'Add function':'Edit function'}><div className="section-title"><h2>{isNew?'Add function':'Edit function'}</h2><button onClick={()=>setEditor(null)} disabled={saving}>Close</button></div>
      <label className="field">Menu label<input autoFocus value={editor.label} onChange={e=>setEditor({...editor,label:e.target.value})}/></label>
      <div className="editor-row"><label className="field">Applies to<select value={editor.appliesTo} onChange={e=>setEditor({...editor,appliesTo:e.target.value as FunctionEntry['appliesTo']})}><option value="file">Files</option><option value="folder">Folders</option><option value="any">Files and folders</option></select></label>
      <label className="field">File extensions <small>(blank = all)</small><input value={editorExtensions} onChange={e=>setEditorExtensions(e.target.value)} placeholder=".mp4, .mov"/></label></div>
      <label className="field">Program or script<input value={editor.executable} onChange={e=>setEditor({...editor,executable:e.target.value})} placeholder="Full path to a program, or {cli}"/></label>
      <button onClick={async()=>{const p=await open({multiple:false,title:'Choose program'});if(typeof p==='string')setEditor({...editor,executable:p});}}>Choose program…</button>
      <label className="field">Arguments <small>(one per line; no surrounding quotes)</small><textarea rows={5} value={editor.args.join('\n')} onChange={e=>setEditor({...editor,args:e.target.value.split('\n')})}/></label>
      <p className="hint">Use {'{input}'} for the selected path, {'{parent}'} for its parent folder, {'{stem}'} for its name without extension, and {'{tools}'} for the installed tools folder. Run PowerShell scripts through powershell.exe with -File.</p>
      <label className="enable"><input type="checkbox" checked={editor.enabled} onChange={e=>setEditor({...editor,enabled:e.target.checked})}/> Enabled</label>
      {error&&<div className="error" role="alert">{error}</div>}
      <div className="editor-actions">{!isNew&&<button className="danger" disabled={saving} onClick={async()=>{if(await persist({...config,functions:config.functions.filter(f=>f.id!==editor.id)})){setEditor(null);setSelected(config.functions.find(f=>f.id!==editor.id)?.id||'');}}}>Remove function</button>}<button className="primary" disabled={saving||!editor.label.trim()||!editor.executable.trim()} onClick={saveEditor}>{saving?'Saving…':'Save function'}</button></div>
    </section></div>}
  </main>;
}
createRoot(document.getElementById('root')!).render(<App/>);

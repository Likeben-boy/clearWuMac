import {useState} from 'react';
import {call,errorText,native,useQuery} from '../api';
import type {AiSettings} from '../types';
import Icon from './Icon';

export default function DeepSeekSettings(){
  const [revision,setRevision]=useState(0);
  const settings=useQuery<AiSettings>('get_ai_settings',{},native,revision);
  const [key,setKey]=useState('');
  const [busy,setBusy]=useState(false);
  const [message,setMessage]=useState('');
  const [error,setError]=useState('');
  async function update(remove=false){
    setBusy(true);setError('');setMessage('');
    try{
      await call<AiSettings>(remove?'remove_deepseek_key':'save_deepseek_key',remove?{}:{key});
      setKey('');setRevision(v=>v+1);
      setMessage(remove?'已移除本机保存的 Key。':'已保存到钥匙串；有效性将在首次解读时检查。');
    }catch(e){setError(errorText(e));}finally{setBusy(false);}
  }
  return <section className="ai-settings" aria-labelledby="deepseek-title">
    <div className="section-line"><h2 id="deepseek-title"><Icon name="sparkles"/>DeepSeek AI 解读</h2><span className="badge">{settings.loading?'正在读取':settings.data?.configured?'已保存 Key':'未配置'}</span></div>
    <p>在文件或文件夹旁点击「AI 解读」，了解它可能的用途、来源和管理方式。使用 {settings.data?.model??'deepseek-flash'}，请求按你的 DeepSeek 账户计费。</p>
    <form onSubmit={e=>{e.preventDefault();void update();}}>
      <label htmlFor="deepseek-key">DeepSeek API Key</label>
      <input id="deepseek-key" type="password" value={key} onChange={e=>setKey(e.target.value)} autoComplete="off" spellCheck={false} maxLength={512} placeholder={settings.data?.configured?'输入新 Key 以替换已保存的 Key':'输入你的 DeepSeek API Key'} disabled={!native||busy}/>
      <p className="small muted">保存在 macOS 钥匙串中，已保存的 Key 不会回显。保存操作不会发起 AI 请求。</p>
      <div className="ai-settings-actions"><button type="submit" className="btn primary" disabled={!native||busy||!key.trim()}>{busy?<Icon name="spinner" spin/>:<Icon name="lock"/>}保存 Key</button><button type="button" className="btn" disabled={!settings.data?.configured||busy} onClick={()=>void update(true)}>移除 Key</button></div>
    </form>
    {(error||settings.error)&&<div className="query-error" role="alert">{error||settings.error}<button className="btn" onClick={()=>setRevision(v=>v+1)}>重新读取</button></div>}
    {message&&<p className="ai-status" role="status">{message}</p>}
    <div className="info-box"><strong>由你决定何时发送</strong>每次解读前会显示将发送给 DeepSeek 的名称、路径、类型和大小。当前用户主目录替换为 ~；其余路径可能包含私人信息。不读取文件正文，也不发送目录下的文件清单。</div>
  </section>;
}

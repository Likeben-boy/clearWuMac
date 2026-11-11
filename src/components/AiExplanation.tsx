import {useEffect,useRef,useState} from 'react';
import {call,errorText,useQuery} from '../api';
import {bytes} from '../domain';
import type {AiSettings,AiPreview,AiAnswer,Entry} from '../types';
import Icon from './Icon';
import Modal from './Modal';

export default function AiExplanation({scanId,entry,onClose,onSettings}:{scanId:string;entry:Entry;onClose:()=>void;onSettings:()=>void}){
  const [revision,setRevision]=useState(0);
  const preview=useQuery<AiPreview>('prepare_ai_explanation',{scanId,entryId:entry.id},true,revision);
  const settings=useQuery<AiSettings>('get_ai_settings',{},true,revision);
  const [answer,setAnswer]=useState<AiAnswer|null>(null);
  const [busy,setBusy]=useState(false),[error,setError]=useState('');
  const alive=useRef(true),sending=useRef(false);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
  async function explain(){
    if(!preview.data||sending.current)return;
    sending.current=true;setBusy(true);setError('');
    try{const result=await call<AiAnswer>('explain_entry',{requestId:preview.data.request_id});if(alive.current)setAnswer(result);}
    catch(e){if(alive.current)setError(errorText(e));}
    finally{sending.current=false;if(alive.current)setBusy(false);}
  }
  const metadata=preview.data?.metadata;
  return <Modal title="AI 解读" onClose={onClose}>
    <div className="ai-target"><Icon name={entry.kind==='directory'?'folder':'file'}/><strong>{entry.name}</strong></div>
    {(preview.loading||settings.loading)&&<p role="status">正在准备信息预览…</p>}
    {(preview.error||settings.error)&&<div className="query-error" role="alert">{preview.error||settings.error}<button className="btn" onClick={()=>setRevision(v=>v+1)}>重新读取</button></div>}
    {metadata&&<>
      <p className="ai-disclosure">点击「发送并解读」后，以下扫描元数据将发送至 DeepSeek（api.deepseek.com）。不读取文件正文或子项清单，按你的 API 账户计费。</p>
      <dl className="ai-metadata"><div><dt>名称</dt><dd>{metadata.name}</dd></div><div><dt>路径</dt><dd>{metadata.path}</dd></div><div><dt>类型</dt><dd>{metadata.kind==='directory'?'文件夹':'文件'}</dd></div><div><dt>逻辑大小</dt><dd>{bytes(metadata.logical_bytes)}</dd></div><div><dt>分配空间</dt><dd>{bytes(metadata.allocated_bytes)}</dd></div><div><dt>统计状态</dt><dd>{metadata.excluded?'按策略跳过':metadata.partial?'部分内容未知':'已统计'}</dd></div></dl>
      <p className="small muted">用户主目录已替换为 ~，其他路径保留供判断用途。解读基于扫描时的元数据，可能不准确。</p>
    </>}
    {settings.data&&!settings.data.configured&&<div className="info-box"><strong>先连接 DeepSeek</strong>请在偏好设置中填写 API Key。<button className="btn primary" onClick={onSettings}>前往设置</button></div>}
    {busy&&<div className="ai-loading" role="status"><Icon name="spinner" spin/>正在请 DeepSeek 解读，最长等待约一分钟…</div>}
    {error&&<div className="query-error" role="alert">{error}<button className="btn" onClick={()=>{setError('');setRevision(v=>v+1);}}>重新准备</button><button className="btn" onClick={onSettings}>检查 API 设置</button></div>}
    {answer&&<section className="ai-answer" aria-label="AI 解读结果"><h3><Icon name="sparkles"/>DeepSeek 的解读</h3><div className="ai-answer-text">{answer.text}</div>{answer.truncated&&<p className="small muted">回答达到长度上限，内容可能不完整。</p>}<p className="small muted">{answer.model} · AI 推断仅供参考，不能作为可安全删除的依据。</p></section>}
    {!answer&&<div className="dialog-bottom"><button className="btn" onClick={onClose}>关闭</button><button className="btn primary" onClick={()=>void explain()} disabled={busy||!!error||!preview.data||preview.loading||settings.loading||!settings.data?.configured}><Icon name={busy?'spinner':'sparkles'} spin={busy}/>发送并解读</button></div>}
  </Modal>;
}

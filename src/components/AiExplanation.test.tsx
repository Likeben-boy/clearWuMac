// @vitest-environment jsdom
import {act} from 'react';
import {createRoot,type Root} from 'react-dom/client';
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {invoke} from '@tauri-apps/api/core';
import type {Entry} from '../types';
import AiExplanation from './AiExplanation';
import DeepSeekSettings from './DeepSeekSettings';

vi.mock('@tauri-apps/api/core',()=>({isTauri:()=>true,invoke:vi.fn()}));
const mockInvoke=vi.mocked(invoke);
const entry={id:'2',name:'Caches',kind:'directory'} as Entry;
const preview={request_id:'preview-1',model:'deepseek-flash',destination:'https://api.deepseek.com/chat/completions',metadata:{name:'Caches',path:'~/Library/Caches',kind:'directory',logical_bytes:'4096',allocated_bytes:null,partial:true,excluded:false}};
let host:HTMLDivElement,root:Root;
let configured:boolean;
let handleExplain:()=>Promise<unknown>;
const calls=(command:string)=>mockInvoke.mock.calls.filter(([name])=>name===command);
function button(text:string){const b=[...host.querySelectorAll('button')].find(b=>b.textContent?.includes(text));if(!b)throw Error(`Missing button: ${text}`);return b;}
async function click(text:string){await act(async()=>{button(text).click();});}
beforeEach(()=>{
  (globalThis as typeof globalThis&{IS_REACT_ACT_ENVIRONMENT:boolean}).IS_REACT_ACT_ENVIRONMENT=true;
  HTMLDialogElement.prototype.showModal=function(){this.open=true;};
  HTMLDialogElement.prototype.close=function(){this.open=false;};
  host=document.createElement('div');document.body.append(host);root=createRoot(host);
  configured=true;handleExplain=async()=>({text:'缓存目录',model:'deepseek-flash',truncated:false});
  mockInvoke.mockReset().mockImplementation(async(command)=>{
    if(command==='get_ai_settings')return {configured,model:'deepseek-flash'};
    if(command==='prepare_ai_explanation')return preview;
    if(command==='explain_entry')return handleExplain();
    if(command==='save_deepseek_key'){configured=true;return {configured:true,model:'deepseek-flash'};}
    if(command==='remove_deepseek_key'){configured=false;return {configured:false,model:'deepseek-flash'};}
    throw Error(`Unexpected command ${command}`);
  });
});
afterEach(async()=>{await act(async()=>root.unmount());host.remove();});

it('预览不会发起 AI 请求，未配置时引导设置',async()=>{
  configured=false;const settings=vi.fn();
  await act(async()=>root.render(<AiExplanation scanId="scan" entry={entry} onClose={()=>{}} onSettings={settings}/>));
  expect(host.textContent).toContain('~/Library/Caches');
  expect(host.textContent).toContain('未知');
  expect(button('发送并解读').disabled).toBe(true);
  expect(calls('explain_entry')).toHaveLength(0);
  await click('前往设置');expect(settings).toHaveBeenCalledOnce();
});

it('显式点击才发送，阻止重复请求并将回答作为纯文本呈现',async()=>{
  let finish!:(value:unknown)=>void;handleExplain=()=>new Promise(resolve=>{finish=resolve;});
  await act(async()=>root.render(<AiExplanation scanId="scan" entry={entry} onClose={()=>{}} onSettings={()=>{}}/>));
  expect(calls('explain_entry')).toHaveLength(0);
  await act(async()=>{button('发送并解读').click();button('发送并解读').click();});
  expect(calls('explain_entry')).toEqual([['explain_entry',{requestId:'preview-1'}]]);
  await act(async()=>finish({text:'<img src=x onerror="alert(1)">',model:'deepseek-flash',truncated:true}));
  expect(host.querySelector('img')).toBeNull();
  expect(host.textContent).toContain('<img src=x');
  expect(host.textContent).toContain('内容可能不完整');
});

it('请求失败后只重新准备，不自动重发收费请求',async()=>{
  handleExplain=async()=>{throw {message:'余额不足'};};
  await act(async()=>root.render(<AiExplanation scanId="scan" entry={entry} onClose={()=>{}} onSettings={()=>{}}/>));
  await click('发送并解读');expect(host.textContent).toContain('余额不足');
  expect(button('发送并解读').disabled).toBe(true);
  await click('重新准备');
  expect(calls('prepare_ai_explanation')).toHaveLength(2);
  expect(calls('explain_entry')).toHaveLength(1);
  expect(button('发送并解读').disabled).toBe(false);
});

it('关闭后旧请求的回答不会出现在另一条目的弹窗',async()=>{
  let finish!:(value:unknown)=>void;handleExplain=()=>new Promise(resolve=>{finish=resolve;});
  await act(async()=>root.render(<AiExplanation key="a" scanId="scan" entry={entry} onClose={()=>{}} onSettings={()=>{}}/>));
  await click('发送并解读');
  await act(async()=>root.render(<AiExplanation key="b" scanId="scan" entry={{...entry,id:'3',name:'Projects'}} onClose={()=>{}} onSettings={()=>{}}/>));
  await act(async()=>finish({text:'OLD PRIVATE ANSWER',model:'deepseek-flash',truncated:false}));
  expect(host.textContent).not.toContain('OLD PRIVATE ANSWER');
});

it('Key 使用密码输入，保存后清空且不写入浏览器存储',async()=>{
  configured=false;
  await act(async()=>root.render(<DeepSeekSettings/>));
  const input=host.querySelector('input')!;
  expect(input.type).toBe('password');
  await act(async()=>{
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(input,'sk-fixture-only-123456');
    input.dispatchEvent(new Event('input',{bubbles:true}));
  });
  await click('保存 Key');
  expect(calls('save_deepseek_key')).toEqual([['save_deepseek_key',{key:'sk-fixture-only-123456'}]]);
  expect(input.value).toBe('');
  expect(host.textContent).not.toContain('sk-fixture-only-123456');
  expect(localStorage.length).toBe(0);expect(sessionStorage.length).toBe(0);
  await click('移除 Key');expect(calls('remove_deepseek_key')).toHaveLength(1);
});

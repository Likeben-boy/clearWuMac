import {useEffect,useRef,type ReactNode} from 'react';
import Icon from './Icon';
export default function Modal({title,children,onClose}:{title:string;children:ReactNode;onClose:()=>void}){
  const ref=useRef<HTMLDialogElement>(null);
  useEffect(()=>{ref.current?.showModal();return()=>ref.current?.close();},[]);
  return <dialog ref={ref} onCancel={e=>{e.preventDefault();onClose();}} aria-labelledby="modal-title"><div className="section-line"><h2 id="modal-title">{title}</h2><button className="icon-btn" onClick={onClose} aria-label="关闭"><Icon name="close"/></button></div>{children}</dialog>;
}

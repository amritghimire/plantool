import {useCallback,useEffect,useRef,useState} from 'react';
export function usePolling<T>(load:()=>Promise<T>,interval=10000){
 const [data,setData]=useState<T|null>(null);const [error,setError]=useState('');const [updated,setUpdated]=useState<Date|null>(null);const refreshRef=useRef(()=>{});
 useEffect(()=>{let active=true,sequence=0;setData(null);setError('');const refresh=()=>{const current=++sequence;void load().then(value=>{if(active&&current===sequence){setData(value);setError('');setUpdated(new Date())}}).catch(e=>{if(active&&current===sequence)setError((e as Error).message)});};refreshRef.current=refresh;refresh();const poll=()=>{if(document.visibilityState!=='hidden')refresh()};const timer=window.setInterval(poll,interval);window.addEventListener('focus',poll);return()=>{active=false;window.clearInterval(timer);window.removeEventListener('focus',poll);refreshRef.current=()=>{}}},[load,interval]);
 return {data,error,updated,refresh:useCallback(()=>refreshRef.current(),[])};
}

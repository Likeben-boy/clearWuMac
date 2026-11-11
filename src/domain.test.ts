import {describe,it,expect} from 'vitest';
import {bytes,layout,percentage,coverage,contribution,resetView} from './domain';
import type {Entry,View} from './types';
describe('空间计量展示',()=>{
  it('区分零、未知，保留大于 JS 安全整数的值',()=>{expect(bytes(null)).toBe('未知');expect(bytes('0')).toBe('0 B');expect(bytes('1024')).toBe('1 KiB');expect(bytes('1073741824')).toBe('1 GiB');expect(bytes('1000000000','decimal')).toBe('1 GB');expect(bytes('9223372036854775807')).toBe('8 EiB');});
  it('未知目录不伪装成空目录',()=>{const base={unknown_allocated:'1',observed_allocated:'0',known_allocated:'0'} as Entry;expect(coverage(base,'allocated')).toBe('unknown');expect(contribution(base,'allocated')).toBe('未知');expect(contribution({...base,excluded:true},'allocated')).toBe('按策略跳过');expect(contribution({...base,observed_allocated:'1'},'allocated')).toBe('0 B（部分已知）');expect(coverage({...base,observed_allocated:'1'},'allocated')).toBe('partial');expect(coverage({...base,unknown_allocated:'0'},'allocated')).toBe('known');});
  it('图块面积守恒，零量不参与绘制',()=>{const items=[{id:'1',name:'a',bytes:'9007199254740993'},{id:'2',name:'b',bytes:'9007199254740993'},{id:'3',name:'c',bytes:'0'}];for(const w of [280,640,1000]){const rects=layout(items,w,260);expect(rects).toHaveLength(2);expect(rects.reduce((s,r)=>s+r.width*r.height,0)).toBeCloseTo(w*260);expect(rects[0].width*rects[0].height).toBeCloseTo(w*260/2);}});
  it('零分母不会生成 NaN',()=>{expect(percentage('0','0')).toBe(0);expect(percentage('1','4')).toBe(25);expect(layout([],300,260)).toEqual([]);});
  it('视图变化清除旧游标与分组，保留未更改口径',()=>{const v={metric:'logical',cursor:'old',previousCursors:['before'],groupToken:'group'} as View;const next=resetView(v,{folderId:'2'});expect(next.metric).toBe('logical');expect(next.cursor).toBeNull();expect(next.previousCursors).toEqual([]);expect(next.groupToken).toBeNull();});
});

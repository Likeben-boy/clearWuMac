export type Metric = 'allocated' | 'logical';
export interface AiSettings { configured:boolean; model:string }
export interface AiPreview { request_id:string; model:string; destination:string; metadata:{ name:string; path:string; kind:string; logical_bytes:string|null; allocated_bytes:string|null; partial:boolean; excluded:boolean } }
export interface AiAnswer { text:string; model:string; truncated:boolean }
export type Screen = 'overview' | 'folders' | 'large' | 'access' | 'settings';
export interface Scope { id:string; key:string; path_display:string; available:boolean }
export interface Volume { id:string; name:string; path_display:string; total:string; free:string; available:string; used:string; sampled_at:number; is_home:boolean }
export interface Scan { id:string; scope_key:string; path_display:string; state:string; reason:string|null; started_at:number; ended_at:number|null; entries:string; files:string; errors:string; excluded:string; has_result:boolean; revision:string; root_id:string; current_path:string; elapsed_ms:number }
export interface Entry { id:string; parent_id:string|null; name:string; path_display:string; kind:string; logical:string|null; allocated:string|null; known_logical:string; known_allocated:string; unknown_logical:string; unknown_allocated:string; gaps:string; observed_logical:string; observed_allocated:string; files:string; canonical_id:string|null; excluded:boolean; note:string|null; modified_at:number|null }
export interface Page<T> { scan_id:string; revision:string; items:T[]; next_cursor:string|null }
export interface Detail { entry:Entry; ancestors:Entry[]; canonical:Entry|null; description:string }
export interface ScanError { id:string; entry_id:string|null; path_display:string; code:string; message:string; occurred_at:number }
export interface Settings { unit:'binary'|'decimal'; metric:Metric; version:number }
export interface Bootstrap { scans:Scan[]; scopes:Scope[]; display_scan_id:string|null; active_scan_id:string|null; settings:Settings; home_scope:Scope|null }
export interface Summary { scan:Scan; root:Entry }
export interface DirectoryMap { scan_id:string; revision:string; items:Entry[]; other_count:string; other_bytes:string; group_token:string|null; total:string; unknown_count:string; zero_count:string }
export interface Query { scan_id:string; parent_id:string|null; metric:Metric; search:string; descending:boolean; cursor:string|null; limit:number; group_token:string|null }
export interface View { page:Screen; scanId:string|null; folderId:string; selectedId:string|null; metric:Metric; search:string; descending:boolean; cursor:string|null; previousCursors:(string|null)[]; groupToken:string|null; map:boolean }

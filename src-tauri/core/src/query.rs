use crate::{model::*, store};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::{params, params_from_iter, types::Value, Connection};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Cursor {
    signature: String,
    unknown: bool,
    value: i64,
    id: i64,
}
#[derive(Serialize, Deserialize)]
struct Group {
    scan: String,
    parent: String,
    metric: Metric,
    ids: Vec<i64>,
}
fn encode<T: Serialize>(value: &T) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("serializable cursor"))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    if value.len() > 16384 {
        return Err(AppError::new("cursor_invalid", "游标过长。"));
    }
    URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|s| serde_json::from_slice(&s).ok())
        .ok_or_else(|| AppError::new("cursor_invalid", "查询游标已失效。"))
}
pub fn list(db: &Connection, q: &Query, mode: &str) -> Result<Page<Entry>> {
    store::require_result(db, &q.scan_id)?;
    if q.limit == 0 || q.limit > 200 || q.search.len() > 1024 {
        return Err(AppError::new("invalid_argument", "查询范围超出限制。"));
    }
    let parent = q.parent_id.as_deref().unwrap_or("1");
    let mut conditions = vec!["scan=?".to_string()];
    let mut args: Vec<Value> = vec![q.scan_id.clone().into()];
    match mode {
        "children" => {
            let entry = store::entry(db, &q.scan_id, id(parent)?)?;
            if entry.kind != "directory" {
                return Err(AppError::new("invalid_argument", "项目不是目录。"));
            }
            conditions.push("parent=?".into());
            args.push(id(parent)?.into());
        }
        "large" => conditions.push("kind='file' AND canonical=id AND excluded=0".into()),
        "aliases" => {
            let e = store::entry(db, &q.scan_id, id(parent)?)?;
            conditions.push("canonical=?".into());
            args.push(id(e.canonical_id.as_deref().unwrap_or(parent))?.into());
        }
        _ => return Err(AppError::new("invalid_argument", "无效查询模式。")),
    }
    if !q.search.trim().is_empty() {
        conditions.push("instr(search,?)>0".into());
        args.push(q.search.trim().to_lowercase().into());
    }
    if let Some(token) = &q.group_token {
        let g: Group = decode(token)?;
        if mode != "children"
            || g.scan != q.scan_id
            || g.parent != parent
            || g.metric != q.metric
            || g.ids.len() != 40
        {
            return Err(AppError::new("cursor_invalid", "分组已失效。"));
        }
        conditions.push(format!("id NOT IN ({})", vec!["?"; g.ids.len()].join(",")));
        args.extend(g.ids.into_iter().map(Value::Integer));
    }
    let (size, unknown) = match q.metric {
        Metric::Allocated => ("known_a", "(unknown_a>0 AND observed_a=0)"),
        Metric::Logical => ("known_l", "(unknown_l>0 AND observed_l=0)"),
    };
    let mut sig_q = q.clone();
    sig_q.cursor = None;
    let signature = serde_json::to_string(&(mode, &sig_q)).unwrap();
    if let Some(cursor) = &q.cursor {
        let c: Cursor = decode(cursor)?;
        if c.signature != signature {
            return Err(AppError::new(
                "cursor_invalid",
                "分页条件已经改变，请重新查询。",
            ));
        }
        let op = if q.descending { "<" } else { ">" };
        conditions.push(format!(
            "({unknown}>? OR ({unknown}=? AND ({size}{op}? OR ({size}=? AND id>?))))"
        ));
        args.extend([
            Value::Integer(c.unknown as i64),
            Value::Integer(c.unknown as i64),
            Value::Integer(c.value),
            Value::Integer(c.value),
            Value::Integer(c.id),
        ]);
    }
    let direction = if q.descending { "DESC" } else { "ASC" };
    let sql = format!(
        "SELECT {} FROM entries WHERE {} ORDER BY {unknown} ASC,{size} {direction},id ASC LIMIT ?",
        store::ENTRY_COLUMNS,
        conditions.join(" AND ")
    );
    args.push((q.limit as i64 + 1).into());
    let mut stmt = db.prepare(&sql)?;
    let mut items = stmt
        .query_map(params_from_iter(args.iter()), store::read_entry)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let more = items.len() > q.limit as usize;
    items.truncate(q.limit as usize);
    let next_cursor = if more {
        items.last().map(|e| {
            encode(&Cursor {
                signature,
                unknown: e.unknown(q.metric),
                value: e.value(q.metric),
                id: id(&e.id).unwrap(),
            })
        })
    } else {
        None
    };
    Ok(Page {
        scan_id: q.scan_id.clone(),
        revision: "1".into(),
        items,
        next_cursor,
    })
}
pub fn map(db: &Connection, sid: &str, parent: &str, metric: Metric) -> Result<DirectoryMap> {
    let q = Query {
        scan_id: sid.into(),
        parent_id: Some(parent.into()),
        metric,
        search: String::new(),
        descending: true,
        cursor: None,
        limit: 40,
        group_token: None,
    };
    let top = list(db, &q, "children")?.items;
    let mut other_bytes = 0i64;
    let mut other_count = 0i64;
    let mut total = 0i64;
    let mut unknown = 0i64;
    let mut zero = 0i64;
    let mut stmt = db.prepare(&format!(
        "SELECT {} FROM entries WHERE scan=?1 AND parent=?2",
        store::ENTRY_COLUMNS
    ))?;
    let mut rows = stmt.query(params![sid, id(parent)?])?;
    let top_ids: Vec<i64> = top.iter().map(|e| id(&e.id).unwrap()).collect();
    while let Some(row) = rows.next()? {
        let e = store::read_entry(row)?;
        let value = e.value(metric);
        if e.unknown(metric) {
            unknown += 1;
        } else if value == 0 {
            zero += 1;
        }
        total = total
            .checked_add(value)
            .ok_or_else(|| AppError::new("size_overflow", "当前目录无法以整数准确绘制。"))?;
        if !top_ids.contains(&id(&e.id)?) {
            other_count += 1;
            other_bytes = other_bytes
                .checked_add(value)
                .ok_or_else(|| AppError::new("size_overflow", "分组大小溢出。"))?;
        }
    }
    let group_token = if other_count > 0 {
        Some(encode(&Group {
            scan: sid.into(),
            parent: parent.into(),
            metric,
            ids: top_ids,
        }))
    } else {
        None
    };
    Ok(DirectoryMap {
        scan_id: sid.into(),
        revision: "1".into(),
        items: top,
        other_count: other_count.to_string(),
        other_bytes: other_bytes.to_string(),
        group_token,
        total: total.to_string(),
        unknown_count: unknown.to_string(),
        zero_count: zero.to_string(),
    })
}
pub fn detail(db: &Connection, sid: &str, eid: &str) -> Result<Detail> {
    store::require_result(db, sid)?;
    let entry = store::entry(db, sid, id(eid)?)?;
    let mut ancestors = Vec::new();
    let mut parent = entry.parent_id.clone();
    while let Some(p) = parent {
        let e = store::entry(db, sid, id(&p)?)?;
        parent = e.parent_id.clone();
        ancestors.push(e);
        if ancestors.len() > 130 {
            break;
        }
    }
    ancestors.reverse();
    let canonical = entry
        .canonical_id
        .as_ref()
        .filter(|c| *c != &entry.id)
        .map(|c| store::entry(db, sid, id(c)?))
        .transpose()?;
    let description = description(&entry);
    Ok(Detail {
        entry,
        ancestors,
        canonical,
        description,
    })
}
pub fn errors(db: &Connection, sid: &str, cursor: Option<String>) -> Result<Page<ScanError>> {
    store::scan(db, sid)?;
    let after = cursor
        .as_ref()
        .map(|c| decode::<(String, i64)>(c))
        .transpose()?;
    if after.as_ref().is_some_and(|(scan, _)| scan != sid) {
        return Err(AppError::new("cursor_invalid", "错误列表游标已失效。"));
    }
    let mut q=db.prepare("SELECT id,entry,path_display,code,message,occurred FROM scan_errors WHERE scan=?1 AND id>?2 ORDER BY id LIMIT 101")?;
    let mut items = q
        .query_map(params![sid, after.map(|(_, n)| n).unwrap_or(0)], |r| {
            Ok(ScanError {
                id: r.get::<_, i64>(0)?.to_string(),
                entry_id: r.get::<_, Option<i64>>(1)?.map(|n| n.to_string()),
                path_display: r.get(2)?,
                code: r.get(3)?,
                message: r.get(4)?,
                occurred_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let more = items.len() > 100;
    items.truncate(100);
    let next_cursor = if more {
        items.last().map(|e| encode(&(sid, id(&e.id).unwrap())))
    } else {
        None
    };
    Ok(Page {
        scan_id: sid.into(),
        revision: "1".into(),
        items,
        next_cursor,
    })
}

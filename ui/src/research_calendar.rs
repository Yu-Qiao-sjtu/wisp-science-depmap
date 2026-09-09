//! Home-level calendar over the same recorded, mainline events as each project.
use crate::app_support::compose_icon;
use crate::dto::{ProjectSummary, ProjectTransferProgress, ResearchCalendarProject};
use crate::i18n::Locale;
use crate::research_journey::{
    call, category, clock, date, day_key, days, j, month_of, month_start, now, shift_month, status,
};
use leptos::*;
use std::collections::HashSet;

fn color(id: &str) -> String {
    // Stable across project reorder, rename, filtering and app restarts.
    let hash = id.bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u32)
    });
    format!("--calendar-project:var(--calendar-color-{})", hash % 6)
}

fn day_start(ts: i64) -> i64 {
    let d = date(ts);
    (js_sys::Date::new_with_year_month_day(
        d.get_full_year(),
        d.get_month() as i32,
        d.get_date() as i32,
    )
    .get_time()
        / 1000.0) as i64
}
fn day_end(ts: i64) -> i64 {
    let d = date(ts);
    (js_sys::Date::new_with_year_month_day(
        d.get_full_year(),
        d.get_month() as i32,
        d.get_date() as i32 + 1,
    )
    .get_time()
        / 1000.0) as i64
}

#[component]
pub(crate) fn ResearchCalendar(
    locale: RwSignal<Locale>,
    projects: Signal<Vec<ProjectSummary>>,
    on_open_journey: Callback<(String, i64)>,
    on_close: Callback<()>,
    project_transfer: ReadSignal<Option<ProjectTransferProgress>>,
) -> impl IntoView {
    let month = create_rw_signal(month_of(now()));
    let selected = create_rw_signal(day_start(now()));
    let filter = create_rw_signal(None::<String>);
    let refresh = create_rw_signal(0u32);
    let project_keys = move || {
        projects
            .get()
            .into_iter()
            .map(|p| (p.id, p.updated_at, p.running_count, p.needs_you_count))
            .collect::<Vec<_>>()
    };
    let history = create_local_resource(
        move || (month.get(), project_keys(), refresh.get()),
        move |(m, keys, _)| async move {
            let ids: Vec<_> = keys.into_iter().map(|p| p.0).collect();
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            call::<Vec<ResearchCalendarProject>>("get_research_calendar", serde_json::json!({"projectIds":ids,"from":month_start(m),"until":month_start(shift_month(m,1))})).await
        },
    );
    // A separate bounded day read keeps drill-down usable when a busy month
    // exceeds the per-project event limit; it also respects 23/25-hour days.
    let daily = create_local_resource(
        move || (selected.get(), project_keys(), refresh.get()),
        move |(ts, keys, _)| async move {
            let ids: Vec<_> = keys.into_iter().map(|p| p.0).collect();
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            call::<Vec<ResearchCalendarProject>>(
                "get_research_calendar",
                serde_json::json!({"projectIds":ids,"from":ts,"until":day_end(ts)}),
            )
            .await
        },
    );
    create_effect(move |_| {
        if filter
            .get()
            .is_some_and(|id| !projects.get().iter().any(|p| p.id == id))
        {
            filter.set(None);
        }
    });
    let included = move |id: &str| {
        projects.with(|ps| ps.iter().any(|p| p.id == id))
            && filter.with(|f| f.as_deref().is_none_or(|f| f == id))
    };
    let change_month = move |delta| {
        let next = shift_month(month.get_untracked(), delta);
        month.set(next);
        selected.set(if next == month_of(now()) {
            day_start(now())
        } else {
            month_start(next)
        });
    };
    view! {
        <section class="home-calendar" data-testid="home-research-calendar" aria-label=move || j(locale.get(),"Research calendar","研究日历")>
            <button type="button" class="calendar-back" on:click=move |_|on_close.call(())>{compose_icon("arrow-left")}{move ||j(locale.get(),"Back to home","返回首页")}</button>
            <header class="home-calendar-heading"><div><h2>{move || j(locale.get(),"Research calendar","研究日历")}</h2><p>{move || j(locale.get(),"Recorded research across your projects, day by day.","把每个项目的探索，放回同一条时间线。")}</p></div>
                <button type="button" class="calendar-icon" aria-label=move || j(locale.get(),"Refresh calendar","刷新日历") on:click=move |_| refresh.update(|n| *n += 1)>{compose_icon("refresh")}</button>
            </header>
            <div class="home-calendar-layout">
                <nav class="calendar-projects" aria-label=move || j(locale.get(),"Calendar projects","日历项目范围")>
                    <button type="button" aria-pressed=move || filter.get().is_none().to_string() on:click=move |_| filter.set(None)>{compose_icon("folder")}{move || j(locale.get(),"All projects","所有项目")}</button>
                    {move || projects.get().into_iter().map(|p| {
                        let id = p.id.clone(); let chosen = p.id.clone();
                        view! {<button type="button" style=color(&p.id) data-project-id=p.id aria-pressed=move || (filter.get().as_ref()==Some(&chosen)).to_string() on:click=move |_|filter.set(Some(id.clone()))><span class="calendar-dot"></span><span>{p.name}</span></button>}
                    }).collect_view()}
                </nav>
                <div class="calendar-workspace">
                    <div class="calendar-toolbar"><div class="calendar-month-controls"><strong data-testid="calendar-month">{move || {let(y,m)=month.get(); if locale.get()==Locale::Zh {format!("{y}年 {}月",m+1)} else {format!("{y} / {:02}",m+1)}}}</strong>
                        <button type="button" class="calendar-icon" aria-label=move ||j(locale.get(),"Previous month","上个月") on:click=move |_|change_month(-1)>{compose_icon("chevron-left")}</button>
                        <button type="button" class="calendar-icon" aria-label=move ||j(locale.get(),"Next month","下个月") on:click=move |_|change_month(1)>{compose_icon("chevron-right")}</button>
                        <button type="button" class="calendar-today" on:click=move |_|{month.set(month_of(now()));selected.set(day_start(now()));}>{move ||j(locale.get(),"Today","今天")}</button>
                    </div><span class="calendar-scope">{move || filter.get().and_then(|id| projects.get().into_iter().find(|p|p.id==id).map(|p|p.name)).unwrap_or_else(||j(locale.get(),"All projects","所有项目").into())}</span></div>
                    <div class="calendar-content">
                        <div class="calendar-month">
                            {move || history.loading().get().then(||view!{<p class="calendar-notice" role="status">{j(locale.get(),"Loading activity…","正在读取研究活动…")}</p>})}
                            {move || match history.get() {
                                Some(Err(e)) if !history.loading().get() => view!{<p class="calendar-notice calendar-error" role="alert">{e}<button type="button" on:click=move |_|refresh.update(|n|*n+=1)>{j(locale.get(),"Try again","重试")}</button></p>}.into_view(),
                                Some(Ok(rows)) if !history.loading().get() => rows.into_iter().filter(|r|included(&r.project_id)).filter_map(|r|{
                                    let name=projects.get().into_iter().find(|p|p.id==r.project_id)?.name;
                                    if let Some(error)=r.error {Some(view!{<p class="calendar-notice calendar-error" role="alert">{format!("{name}: {error}")}</p>}.into_view())}
                                    else if r.history.truncated {Some(view!{<p class="calendar-notice">{format!("{name}: {}",j(locale.get(),"Showing the latest 2,000 events; select a date for a day-level read. Unmarked dates may have more activity.","仅展示最近 2,000 条活动；请选择日期按日读取，未标记日期仍可能有活动。"))}</p>}.into_view())}else{None}
                                }).collect_view(),
                                _ => ().into_view(),
                            }}
                            <div class="calendar-week">{move || {let labels=if locale.get()==Locale::Zh {["一","二","三","四","五","六","日"]}else{["Mon","Tue","Wed","Thu","Fri","Sat","Sun"]};labels.into_iter().map(|s|view!{<span>{s}</span>}).collect_view()}}</div>
                            <div class="calendar-grid" data-testid="home-calendar-grid">
                                {move || {
                                    let m=month.get(); let offset=(date(month_start(m)).get_day()+6)%7;
                                    let count=js_sys::Date::new_with_year_month_day(m.0 as u32,m.1 as i32+1,0).get_date();
                                    let rows=if history.loading().get(){vec![]}else{history.get().and_then(Result::ok).unwrap_or_default()};
                                    let marks: Vec<_>=rows.iter().filter(|r|included(&r.project_id)&&r.error.is_none()).map(|r|(r.project_id.clone(),r.history.entries.iter().map(|e|day_key(e.occurred_at)).collect::<HashSet<_>>())).collect();
                                    let cells=(0..(offset+count).div_ceil(7)*7).map(|cell| {
                                        if cell<offset || cell>=offset+count {return view!{<span class="calendar-blank"></span>}.into_view();}
                                        let d=cell-offset+1; let ts=(js_sys::Date::new_with_year_month_day(m.0 as u32,m.1 as i32,d as i32).get_time()/1000.0) as i64; let key=day_key(ts);
                                        let ids:Vec<_>=marks.iter().filter(|(_,days)|days.contains(&key)).map(|(id,_)|id.clone()).collect();
                                        let names=projects.get().iter().filter(|p|ids.contains(&p.id)).map(|p|p.name.clone()).collect::<Vec<_>>().join(", ");
                                        let description=if names.is_empty(){None}else{Some(names)};
                                        view!{<button type="button" class="calendar-day" class:selected=move ||selected.get()==ts class:today=ts==day_start(now()) aria-label=key.clone() aria-description=description aria-pressed=move ||(selected.get()==ts).to_string() data-date=key on:click=move |_|selected.set(ts)><span class="calendar-number">{d}</span><span class="calendar-marks">{ids.into_iter().map(|id|view!{<span class="calendar-dot" style=color(&id) data-project-id=id></span>}).collect_view()}</span></button>}.into_view()
                                    }).collect_view();
                                    cells
                                }}
                            </div>
                            <div class="calendar-legend">{move ||projects.get().into_iter().filter(|p|included(&p.id)).map(|p|view!{<span style=color(&p.id)><i class="calendar-dot"></i>{p.name}</span>}).collect_view()}</div>
                        </div>
                        <aside class="calendar-details" data-testid="home-calendar-details" aria-live="polite">
                            <h3>{move ||day_key(selected.get())}{move ||(selected.get()==day_start(now())).then(||j(locale.get()," · Today"," · 今天"))}</h3>
                            {move || {
                                let loc=locale.get();
                                if daily.loading().get(){return view!{<p class="calendar-empty" role="status">{j(loc,"Loading records…","正在读取当天记录…")}</p>}.into_view();}
                                let rows=match daily.get(){Some(Ok(rows))=>rows,Some(Err(e))=>return view!{<p class="calendar-error" role="alert">{e}<button type="button" on:click=move |_|refresh.update(|n|*n+=1)>{j(loc,"Try again","重试")}</button></p>}.into_view(),None=>return ().into_view()};
                                let rows:Vec<_>=rows.into_iter().filter(|r|included(&r.project_id)).collect();
                                let has_errors=rows.iter().any(|r|r.error.is_some());
                                let groups:Vec<_>=rows.into_iter().filter_map(|r|{
                                    let name=projects.get().into_iter().find(|p|p.id==r.project_id)?.name;
                                    let entries=days(&r.history.entries,"").into_iter().flat_map(|(_,entries)|entries).collect::<Vec<_>>();
                                    if entries.is_empty()&&r.error.is_none(){return None;}
                                    Some((r.project_id,name,entries,r.error,r.history.truncated))
                                }).collect();
                                if groups.is_empty(){return view!{<p class="calendar-empty">{if projects.get().is_empty(){j(loc,"Create a project to begin recording research activity.","创建项目后，已记录的研究活动会出现在这里。")}else{j(loc,"No recorded activity on this date.","当天没有已记录的研究活动。")}}</p>}.into_view();}
                                let count=groups.iter().map(|g|g.2.len()).sum::<usize>();
                                let active=groups.iter().filter(|g|!g.2.is_empty()).count();
                                let partial=has_errors||groups.iter().any(|g|g.4);
                                view!{<p class="calendar-detail-meta">{format!("{}{} · {} {}",if partial{j(loc,"Loaded: ","已读取：")}else{""},if loc==Locale::Zh{format!("{active} 个项目")}else{format!("{active} projects")},count,j(loc,"records","条记录"))}</p>
                                    {groups.into_iter().map(|(id,name,entries,error,truncated)|{
                                        let open_id=id.clone();let locked=id.clone();
                                        view!{<section class="calendar-record-group" style=color(&id) data-project-id=id>
                                            <button type="button" class="calendar-project-link" aria-label=format!("{} · {}",name,j(loc,"Research journey","研究历程")) disabled=move ||project_transfer.get().is_some_and(|t|t.is_exporting_project(&locked)) on:click=move |_|on_open_journey.call((open_id.clone(),selected.get_untracked()))><span class="calendar-dot"></span><span>{name}</span>{compose_icon("external-link")}</button>
                                            {error.map(|e|view!{<p class="calendar-error" role="alert">{e}</p>})}
                                            {truncated.then(||view!{<p class="calendar-notice">{j(loc,"Latest 2,000 events shown; more records exist on this day.","当前展示当天最近 2,000 条活动，还有更多记录。")}</p>})}
                                            {entries.into_iter().rev().map(|e|{
                                                let label=format!("{}{}{}",category(loc,&e.kind),if e.kind=="run"{format!(" · {}",status(loc,&e.status))}else{String::new()},if e.manual{j(loc," · Manual"," · 手动")}else{""});
                                                let title=if let Some(v)=e.version_number{format!("{} · v{v}",e.title)}else{e.title};
                                                view!{<article class="calendar-record"><div class="calendar-record-meta"><time>{clock(e.occurred_at)}</time><span class:calendar-error=e.status=="failed"||e.status=="lost">{label}</span></div><p>{title}</p></article>}
                                            }).collect_view()}
                                        </section>}
                                    }).collect_view()}
                                }.into_view()
                            }}
                        </aside>
                    </div>
                    <footer class="calendar-footer"><span>{move ||{
                        if history.loading().get(){return String::new();}
                        let Some(Ok(rows))=history.get() else{return String::new();};
                        let rows:Vec<_>=rows.iter().filter(|r|included(&r.project_id)).collect();
                        let partial=rows.iter().any(|r|r.error.is_some()||r.history.truncated);
                        let active=rows.iter().filter(|r|!r.history.entries.is_empty()).count();
                        let days=rows.iter().flat_map(|r|r.history.entries.iter().map(|e|day_key(e.occurred_at))).collect::<HashSet<_>>().len();
                        if locale.get()==Locale::Zh{format!("{} · {active} 个活跃项目 · {days} 个活动日",if partial{"本月已读取"}else{"本月"})}else{format!("{} · {active} active projects · {days} active days",if partial{"Loaded this month"}else{"This month"})}
                    }}</span><span>{move ||j(locale.get(),"Recorded mainline activity · Local timezone","仅展示主线已记录活动 · 日期按本地时区")}</span></footer>
                </div>
            </div>
        </section>
    }
}

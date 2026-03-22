use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{info, warn};
use serde::Deserialize;
use sqlx::Row;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use crate::startup::AppState;

/// Check if the request IP is in one of the configured allowed subnets
fn is_admin_allowed(addr: &SocketAddr, allowed_subnets: &[String]) -> bool {
    let ip = addr.ip();
    for subnet in allowed_subnets {
        if let Some(ok) = matches_cidr(&ip, subnet) {
            if ok {
                return true;
            }
        }
    }
    false
}

/// Simple CIDR matching: "10.100.0.0/24" or "127.0.0.1/32"
fn matches_cidr(ip: &IpAddr, cidr: &str) -> Option<bool> {
    let parts: Vec<&str> = cidr.split('/').collect();
    let subnet_ip: IpAddr = parts.first()?.parse().ok()?;
    let prefix_len: u32 = parts.get(1)?.parse().ok()?;

    match (ip, subnet_ip) {
        (IpAddr::V4(ip4), IpAddr::V4(sub4)) => {
            let mask = if prefix_len == 0 {
                0u32
            } else {
                !0u32 << (32 - prefix_len)
            };
            Some(u32::from(*ip4) & mask == u32::from(sub4) & mask)
        }
        (IpAddr::V6(ip6), IpAddr::V6(sub6)) => {
            let mask = if prefix_len == 0 {
                0u128
            } else {
                !0u128 << (128 - prefix_len)
            };
            Some(u128::from(*ip6) & mask == u128::from(sub6) & mask)
        }
        _ => None,
    }
}

pub async fn admin_dashboard(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, Response> {
    if !is_admin_allowed(&addr, &state.settings.admin.allowed_subnets) {
        return Err((StatusCode::FORBIDDEN, "Not authorized").into_response());
    }

    // Today's date
    let today = time::OffsetDateTime::now_utc().date().to_string();

    // Gather stats
    let total_users = query_count(&state, "SELECT COUNT(*) as c FROM users").await;
    let today_sessions = query_count(
        &state,
        &format!("SELECT COUNT(*) as c FROM game_sessions WHERE start_time >= '{today}'"),
    )
    .await;
    let today_scores = query_count(
        &state,
        &format!("SELECT COUNT(*) as c FROM scores WHERE created_at >= '{today}'"),
    )
    .await;
    let today_payments = query_count(
        &state,
        &format!(
        "SELECT COUNT(*) as c FROM game_payments WHERE status = 'paid' AND created_at >= '{today}'"
    ),
    )
    .await;
    let total_scores = query_count(&state, "SELECT COUNT(*) as c FROM scores").await;
    let total_payments = query_count(
        &state,
        "SELECT COUNT(*) as c FROM game_payments WHERE status = 'paid'",
    )
    .await;
    let flagged_scores = query_count(
        &state,
        "SELECT COUNT(*) as c FROM score_metadata WHERE flags IS NOT NULL AND flags != ''",
    )
    .await;
    let rejected_scores = query_count(
        &state,
        "SELECT COUNT(*) as c FROM score_metadata WHERE rejected = 1",
    )
    .await;

    // Top scores today — column order must match header: Player, Score, Level, Time, When
    let top_scores = query_rows(&state,
        &format!("SELECT u.username, s.score, s.level, s.play_time, s.created_at FROM scores s JOIN users u ON s.user_id = u.id WHERE s.created_at >= '{today}' ORDER BY s.score DESC LIMIT 10")
    ).await;

    // Recent bot flags
    let recent_flags = query_rows(&state,
        "SELECT username, score, flags, created_at FROM score_metadata WHERE flags IS NOT NULL AND flags != '' ORDER BY created_at DESC LIMIT 10"
    ).await;

    // IPs with multiple accounts
    let sus_ips = query_rows(&state,
        "SELECT client_ip, COUNT(DISTINCT user_id) as accounts, COUNT(*) as games FROM score_metadata WHERE client_ip IS NOT NULL GROUP BY client_ip HAVING accounts > 1 ORDER BY accounts DESC LIMIT 10"
    ).await;

    // Prize payouts (include payment_id hash)
    let recent_payouts = query_rows(&state,
        "SELECT pp.date, u.username, pp.score, pp.amount_sats, pp.status, COALESCE(pp.payment_id, '') FROM prize_payouts pp JOIN users u ON pp.user_id = u.id ORDER BY pp.date DESC LIMIT 10"
    ).await;

    // Recent entry fee payments (include payment_id hash)
    let recent_entries = query_rows(&state,
        &format!("SELECT u.username, gp.amount_sats, gp.status, gp.payment_id, gp.created_at FROM game_payments gp JOIN users u ON gp.user_id = u.id WHERE gp.created_at >= '{today}' ORDER BY gp.created_at DESC LIMIT 20")
    ).await;

    // Banned IPs
    let banned_ips = query_rows(
        &state,
        "SELECT ip, COALESCE(reason, ''), banned_at FROM banned_ips ORDER BY banned_at DESC",
    )
    .await;

    // Banned users
    let banned_users = query_rows(&state,
        "SELECT id, username, COALESCE(ban_reason, '') FROM users WHERE banned = 1 ORDER BY updated_at DESC"
    ).await;

    let entry_fee = state.settings.competition_settings.entry_fee_sats;
    let prize_pct = state.settings.competition_settings.prize_pool_pct;
    let house_pct = 100 - prize_pct as i64;
    let today_gross = today_payments * entry_fee;
    let today_revenue = today_gross * house_pct / 100;

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Proof of Score Admin</title>
<style>
body {{ background: #1a1a2e; color: #e0e0e0; font-family: monospace; padding: 20px; margin: 0; }}
h1 {{ color: #00ff88; }}
h2 {{ color: #ffaa00; border-bottom: 1px solid #333; padding-bottom: 5px; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 15px; margin: 20px 0; }}
.card {{ background: #16213e; border: 1px solid #333; border-radius: 8px; padding: 15px; }}
.card .value {{ font-size: 2em; color: #00ff88; }}
.card .label {{ color: #888; font-size: 0.9em; }}
table {{ border-collapse: collapse; width: 100%; margin: 10px 0; }}
th, td {{ text-align: left; padding: 8px; border-bottom: 1px solid #333; }}
th {{ color: #ffaa00; }}
.flag {{ background: #ff444433; color: #ff8888; padding: 2px 6px; border-radius: 3px; font-size: 0.85em; }}
td {{ max-width: 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }}
td:hover {{ overflow: visible; white-space: normal; word-break: break-all; }}
.ok {{ color: #00ff88; }}
.warn {{ color: #ffaa00; }}
.error {{ color: #ff4444; }}
</style>
</head>
<body>
<h1>Proof of Score Admin</h1>
<p>Date: {today} UTC</p>

<div class="grid">
  <div class="card"><div class="value">{total_users}</div><div class="label">Total Users</div></div>
  <div class="card"><div class="value">{today_sessions}</div><div class="label">Today's Sessions</div></div>
  <div class="card"><div class="value">{today_scores}</div><div class="label">Today's Scores</div></div>
  <div class="card"><div class="value">{today_payments}</div><div class="label">Today's Payments</div></div>
  <div class="card"><div class="value">{today_gross}</div><div class="label">Today's Gross (sats)</div></div>
  <div class="card"><div class="value">{today_revenue}</div><div class="label">Today's Revenue (sats, {house_pct}%)</div></div>
  <div class="card"><div class="value">{total_scores}</div><div class="label">All-time Scores</div></div>
  <div class="card"><div class="value">{total_payments}</div><div class="label">All-time Payments</div></div>
  <div class="card"><div class="value">{flagged_scores}</div><div class="label">Flagged Scores</div></div>
  <div class="card"><div class="value {}">{rejected_scores}</div><div class="label">Rejected Scores</div></div>
</div>

<h2>Today's Top Scores</h2>
<table>
<tr><th>Player</th><th>Score</th><th>Level</th><th>Time</th><th>When</th></tr>
{top_scores_html}
</table>

<h2>Recent Bot Flags</h2>
<table>
<tr><th>Player</th><th>Score</th><th>Flags</th><th>When</th></tr>
{flags_html}
</table>

<h2>Suspicious IPs (multiple accounts)</h2>
<table>
<tr><th>IP</th><th>Accounts</th><th>Games</th></tr>
{ips_html}
</table>

<h2>Ban Management</h2>
<div style="display: grid; grid-template-columns: 1fr 1fr; gap: 20px;">
  <div>
    <h3 style="color: #ff4444;">Ban IP</h3>
    <form method="POST" action="/admin/ban-ip" style="display: flex; gap: 8px; flex-wrap: wrap;">
      <input name="ip" placeholder="IP address" style="background: #16213e; color: #fff; border: 1px solid #333; padding: 6px; font-family: monospace;">
      <input name="reason" placeholder="Reason" style="background: #16213e; color: #fff; border: 1px solid #333; padding: 6px; font-family: monospace;">
      <button type="submit" style="background: #ff4444; color: #fff; border: none; padding: 6px 12px; cursor: pointer; font-family: monospace;">Ban</button>
    </form>
    <table style="margin-top: 10px;">
      <tr><th>IP</th><th>Reason</th><th>When</th><th></th></tr>
      {banned_ips_html}
    </table>
  </div>
  <div>
    <h3 style="color: #ff4444;">Ban User</h3>
    <form method="POST" action="/admin/ban-user" style="display: flex; gap: 8px; flex-wrap: wrap;">
      <input name="user_id" type="number" placeholder="User ID" style="background: #16213e; color: #fff; border: 1px solid #333; padding: 6px; width: 80px; font-family: monospace;">
      <input name="reason" placeholder="Reason" style="background: #16213e; color: #fff; border: 1px solid #333; padding: 6px; font-family: monospace;">
      <button type="submit" style="background: #ff4444; color: #fff; border: none; padding: 6px 12px; cursor: pointer; font-family: monospace;">Ban</button>
    </form>
    <table style="margin-top: 10px;">
      <tr><th>ID</th><th>Username</th><th>Reason</th><th></th></tr>
      {banned_users_html}
    </table>
  </div>
</div>

<h2>Today's Entry Payments</h2>
<table>
<tr><th>Player</th><th>Amount</th><th>Status</th><th>Payment Hash</th><th>When</th></tr>
{entries_html}
</table>

<h2>Recent Prize Payouts</h2>
<table>
<tr><th>Date</th><th>Winner</th><th>Score</th><th>Prize (sats)</th><th>Status</th><th>Payment Hash</th></tr>
{payouts_html}
</table>

<h2>Config</h2>
<table>
<tr><td>Entry fee</td><td>{entry_fee} sats</td></tr>
<tr><td>Prize pool</td><td>{prize_pct}%</td></tr>
<tr><td>Competition window</td><td>{start} UTC + {duration}</td></tr>
<tr><td>Bot detection</td><td>{bot_status}</td></tr>
</table>

<p style="color:#555; margin-top:40px;">Auto-refresh: <a href="/admin" style="color:#00ff88;">reload</a></p>
</body>
</html>"#,
        if rejected_scores > 0 { "error" } else { "ok" },
        top_scores_html = render_rows(&top_scores),
        flags_html = render_rows(&recent_flags),
        ips_html = render_rows(&sus_ips),
        entries_html = render_rows(&recent_entries),
        payouts_html = render_rows(&recent_payouts),
        banned_ips_html = render_banned_ips(&banned_ips),
        banned_users_html = render_banned_users(&banned_users),
        start = state.settings.competition_settings.start_time,
        duration = state.settings.competition_settings.duration_display(),
        bot_status = if state.settings.bot_detection.enabled {
            "enabled"
        } else {
            "disabled"
        },
    );

    Ok(Html(html))
}

async fn query_count(state: &Arc<AppState>, sql: &str) -> i64 {
    match sqlx::query(sql)
        .fetch_one(&state.game_store.get_pool())
        .await
    {
        Ok(row) => row.get::<i64, _>("c"),
        Err(e) => {
            warn!("Admin query failed: {}", e);
            0
        }
    }
}

async fn query_rows(state: &Arc<AppState>, sql: &str) -> Vec<Vec<String>> {
    match sqlx::query(sql)
        .fetch_all(&state.game_store.get_pool())
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                (0..row.len())
                    .map(|i| {
                        row.try_get::<String, _>(i)
                            .or_else(|_| row.try_get::<i64, _>(i).map(|v| v.to_string()))
                            .or_else(|_| row.try_get::<f64, _>(i).map(|v| format!("{:.1}", v)))
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect(),
        Err(e) => {
            warn!("Admin query failed: {}", e);
            vec![]
        }
    }
}

fn render_rows(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "<tr><td colspan='10' style='color:#555'>No data</td></tr>".to_string();
    }
    rows.iter()
        .map(|row| {
            let cells: String = row.iter().map(|c| format!("<td>{c}</td>")).collect();
            format!("<tr>{cells}</tr>")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_banned_ips(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "<tr><td colspan='4' style='color:#555'>No banned IPs</td></tr>".to_string();
    }
    rows.iter()
        .map(|row| {
            let ip = row.first().map(|s| s.as_str()).unwrap_or("");
            let reason = row.get(1).map(|s| s.as_str()).unwrap_or("");
            let when = row.get(2).map(|s| s.as_str()).unwrap_or("");
            format!(
                r#"<tr><td>{ip}</td><td>{reason}</td><td>{when}</td><td><form method="POST" action="/admin/unban-ip" style="display:inline"><input type="hidden" name="ip" value="{ip}"><button type="submit" style="background:#00ff88;color:#000;border:none;padding:2px 8px;cursor:pointer;font-family:monospace;font-size:0.85em;">Unban</button></form></td></tr>"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_banned_users(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "<tr><td colspan='4' style='color:#555'>No banned users</td></tr>".to_string();
    }
    rows.iter()
        .map(|row| {
            let id = row.first().map(|s| s.as_str()).unwrap_or("");
            let username = row.get(1).map(|s| s.as_str()).unwrap_or("");
            let reason = row.get(2).map(|s| s.as_str()).unwrap_or("");
            format!(
                r#"<tr><td>{id}</td><td>{username}</td><td>{reason}</td><td><form method="POST" action="/admin/unban-user" style="display:inline"><input type="hidden" name="user_id" value="{id}"><button type="submit" style="background:#00ff88;color:#000;border:none;padding:2px 8px;cursor:pointer;font-family:monospace;font-size:0.85em;">Unban</button></form></td></tr>"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Admin actions ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct BanIpForm {
    pub ip: String,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct UnbanIpForm {
    pub ip: String,
}

#[derive(Deserialize)]
pub struct BanUserForm {
    pub user_id: i64,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct UnbanUserForm {
    pub user_id: i64,
}

pub async fn admin_ban_ip(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Form(form): Form<BanIpForm>,
) -> Result<Redirect, Response> {
    if !is_admin_allowed(&addr, &state.settings.admin.allowed_subnets) {
        return Err((StatusCode::FORBIDDEN, "Not authorized").into_response());
    }
    info!("Admin: banning IP {}: {:?}", form.ip, form.reason);
    state
        .game_store
        .ban_ip(&form.ip, form.reason.as_deref(), Some("admin"))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())?;
    Ok(Redirect::to("/admin"))
}

pub async fn admin_unban_ip(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Form(form): Form<UnbanIpForm>,
) -> Result<Redirect, Response> {
    if !is_admin_allowed(&addr, &state.settings.admin.allowed_subnets) {
        return Err((StatusCode::FORBIDDEN, "Not authorized").into_response());
    }
    info!("Admin: unbanning IP {}", form.ip);
    state
        .game_store
        .unban_ip(&form.ip)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())?;
    Ok(Redirect::to("/admin"))
}

pub async fn admin_ban_user(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Form(form): Form<BanUserForm>,
) -> Result<Redirect, Response> {
    if !is_admin_allowed(&addr, &state.settings.admin.allowed_subnets) {
        return Err((StatusCode::FORBIDDEN, "Not authorized").into_response());
    }
    let reason = form.reason.as_deref().unwrap_or("Banned by admin");
    info!("Admin: banning user_id {}: {}", form.user_id, reason);
    state
        .user_store
        .ban_user(form.user_id, reason)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())?;
    Ok(Redirect::to("/admin"))
}

pub async fn admin_unban_user(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Form(form): Form<UnbanUserForm>,
) -> Result<Redirect, Response> {
    if !is_admin_allowed(&addr, &state.settings.admin.allowed_subnets) {
        return Err((StatusCode::FORBIDDEN, "Not authorized").into_response());
    }
    info!("Admin: unbanning user_id {}", form.user_id);
    state
        .user_store
        .unban_user(form.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())?;
    Ok(Redirect::to("/admin"))
}

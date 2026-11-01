//! The three pages a phone can see: the download page, the upload page and the PIN page.
//! Self-contained: inline CSS and JS, inline SVG, system fonts. The page makes no request
//! except to the Krovi server it came from (the Content-Security-Policy enforces that).
//! Black and white, matching the desktop app; follows the phone's light/dark setting.
//! Every user-controlled string (file names) is HTML-escaped and only ever placed in text
//! nodes or quoted attributes, never in script.

use crate::names::html_escape;

const CSS: &str = r#"
:root{color-scheme:dark light;--bg:#000;--fg:#fff;--fg2:rgba(255,255,255,.62);--card:rgba(255,255,255,.07);--line:rgba(255,255,255,.14);--btn:#fff;--btnfg:#000;--ok:#fff;--bad:#ff8a8a}
@media (prefers-color-scheme:light){:root{--bg:#f4f4f6;--fg:#0b0b0d;--fg2:rgba(10,10,12,.62);--card:#fff;--line:rgba(0,0,0,.12);--btn:#0b0b0d;--btnfg:#fff;--ok:#0b0b0d;--bad:#b42323}}
*{box-sizing:border-box;-webkit-tap-highlight-color:transparent}
html,body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.45 system-ui,-apple-system,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif}
body{min-height:100vh;min-height:100dvh;padding:max(20px,env(safe-area-inset-top)) max(18px,env(safe-area-inset-right)) max(24px,env(safe-area-inset-bottom)) max(18px,env(safe-area-inset-left))}
main{max-width:520px;margin:0 auto}
.brand{display:flex;align-items:center;gap:10px;margin:4px 0 26px;font-weight:650;letter-spacing:-.01em}
.mark{display:grid;place-items:center;width:30px;height:30px;border-radius:9px;background:var(--btn);color:var(--btnfg);font-weight:800;font-size:15px}
.tag{margin-left:auto;font-size:12px;font-weight:600;color:var(--fg2);border:1px solid var(--line);padding:3px 10px;border-radius:99px}
h1{font-size:28px;line-height:1.15;letter-spacing:-.025em;margin:0 0 6px}
.sub{margin:0 0 22px;color:var(--fg2)}
ul{list-style:none;margin:0 0 14px;padding:0;display:grid;gap:12px}
.file{background:var(--card);border:1px solid var(--line);border-radius:20px;padding:16px}
.row{display:flex;align-items:center;gap:12px;margin-bottom:14px}
.ext{flex:none;display:grid;place-items:center;width:46px;height:46px;border-radius:13px;border:1px solid var(--line);font-size:11px;font-weight:700;letter-spacing:.04em;color:var(--fg2);text-transform:uppercase;overflow:hidden}
.meta{min-width:0}
.name{font-weight:600;overflow-wrap:anywhere;word-break:break-word}
.size{font-size:13px;color:var(--fg2)}
.btn{display:flex;align-items:center;justify-content:center;gap:10px;width:100%;min-height:58px;padding:0 20px;border:0;border-radius:16px;background:var(--btn);color:var(--btnfg);font:inherit;font-size:18px;font-weight:700;text-decoration:none;cursor:pointer}
.btn:active{transform:scale(.985);opacity:.9}
.btn.ghost{background:transparent;color:var(--fg);border:1px solid var(--line)}
.btn svg{width:22px;height:22px;flex:none}
.foot{margin:22px 4px 0;font-size:13px;color:var(--fg2);text-align:center}
.bar{height:6px;border-radius:99px;background:var(--line);overflow:hidden;margin-top:10px}
.bar i{display:block;height:100%;width:0;background:var(--fg);border-radius:99px;transition:width .15s linear}
.st{font-size:13px;color:var(--fg2);margin-top:6px}
.st.bad{color:var(--bad)}
.ok{display:grid;place-items:center;gap:10px;text-align:center;padding:30px 10px 10px}
.ok .tick{display:grid;place-items:center;width:72px;height:72px;border-radius:50%;background:var(--btn);color:var(--btnfg)}
.ok .tick svg{width:36px;height:36px}
.ok h2{margin:6px 0 0;font-size:24px;letter-spacing:-.02em}
.pin{width:100%;height:72px;border-radius:18px;border:1px solid var(--line);background:var(--card);color:var(--fg);font:inherit;font-size:34px;font-weight:700;letter-spacing:.5em;text-align:center;padding-left:.5em;margin-bottom:14px}
.err{color:var(--bad);margin:0 0 14px;font-weight:600}
[hidden]{display:none!important}
"#;

const DOWNLOAD_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/></svg>"#;
const UPLOAD_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12"/><path d="m17 8-5-5-5 5"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/></svg>"#;
const CHECK_ICON: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5"/></svg>"#;

fn head(title: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1,viewport-fit=cover\">\
<meta name=\"color-scheme\" content=\"dark light\"><meta name=\"robots\" content=\"noindex,nofollow\">\
<meta name=\"referrer\" content=\"no-referrer\"><title>{}</title><style>{}</style></head>",
        html_escape(title),
        CSS
    )
}

fn brand() -> &'static str {
    "<div class=\"brand\"><span class=\"mark\">K</span><span>Krovi</span><span class=\"tag\">Quick Transfer</span></div>"
}

pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn ext_badge(name: &str) -> String {
    let (_, ext) = crate::names::split_ext(name);
    let e: String = ext.trim_start_matches('.').chars().take(4).collect();
    if e.is_empty() {
        "FILE".to_string()
    } else {
        e.to_uppercase()
    }
}

pub fn expiry_text(secs: u64) -> String {
    match secs {
        0..=59 => "less than a minute".to_string(),
        60..=3599 => {
            let m = secs.div_ceil(60);
            format!("{m} min")
        }
        _ => format!("{} h {} min", secs / 3600, (secs % 3600) / 60),
    }
}

/// The download page. `files` are `(name, size)` in index order.
pub fn send_page(token: &str, files: &[(String, u64)], expires_in_secs: u64) -> String {
    let mut h = head("Krovi · Download");
    h.push_str("<body><main>");
    h.push_str(brand());
    let n = files.len();
    h.push_str(&format!(
        "<h1>{}</h1><p class=\"sub\">Tap Download. The file comes straight from the computer over this Wi-Fi.</p><ul>",
        if n == 1 { "1 file ready".to_string() } else { format!("{n} files ready") }
    ));
    for (i, (name, size)) in files.iter().enumerate() {
        let esc = html_escape(name);
        h.push_str(&format!(
            "<li class=\"file\"><div class=\"row\"><div class=\"ext\">{}</div><div class=\"meta\"><div class=\"name\">{esc}</div><div class=\"size\">{}</div></div></div>\
<a class=\"btn dl\" href=\"/s/{token}/{i}\" download=\"{esc}\">{DOWNLOAD_ICON}<span>Download</span></a></li>",
            html_escape(&ext_badge(name)),
            format_bytes(*size)
        ));
    }
    h.push_str("</ul>");
    if n > 1 {
        h.push_str(&format!(
            "<button class=\"btn ghost\" id=\"all\" type=\"button\">{DOWNLOAD_ICON}<span>Download all ({n})</span></button>"
        ));
    }
    h.push_str(&format!(
        "<p class=\"foot\">Private to this Wi-Fi. Link expires in {}. Nothing goes through the internet.</p></main>",
        expiry_text(expires_in_secs)
    ));
    h.push_str(
        "<script>(function(){var b=document.getElementById('all');if(!b)return;b.addEventListener('click',function(){\
var l=[].slice.call(document.querySelectorAll('a.dl')),i=0;b.disabled=true;(function next(){if(i>=l.length){b.disabled=false;return}l[i++].click();setTimeout(next,900)})()})})();</script></body></html>",
    );
    h
}

/// The upload page. Posts each file as a raw body to `/r/<token>/up?n=<name>`.
pub fn receive_page(token: &str, max_bytes: u64, expires_in_secs: u64) -> String {
    let mut h = head("Krovi · Send to computer");
    h.push_str(&format!(
        "<body data-token=\"{token}\" data-max=\"{max_bytes}\"><main>{}\
<h1>Send to computer</h1><p class=\"sub\">Pick photos, videos or files. They go straight to the computer over this Wi-Fi.</p>\
<button class=\"btn\" id=\"pick\" type=\"button\">{UPLOAD_ICON}<span>Choose files</span></button>\
<input id=\"f\" type=\"file\" multiple hidden>\
<ul id=\"list\" style=\"margin-top:16px\"></ul>\
<div class=\"ok\" id=\"done\" hidden><div class=\"tick\">{CHECK_ICON}</div><h2>Sent to your computer</h2><p class=\"sub\" id=\"sum\"></p></div>\
<p class=\"foot\">Up to {} per link. Link expires in {}. Nothing goes through the internet.</p></main>",
        brand(),
        format_bytes(max_bytes),
        expiry_text(expires_in_secs)
    ));
    h.push_str(RECEIVE_JS);
    h.push_str("</body></html>");
    h
}

const RECEIVE_JS: &str = r#"<script>(function(){
var B=document.body,T=B.dataset.token,MAX=+B.dataset.max;
var $=function(i){return document.getElementById(i)};
var pick=$('pick'),input=$('f'),list=$('list'),done=$('done'),sum=$('sum');
var q=[],busy=false,ok=0,bad=0,bytes=0;
function sz(n){var u=['B','KB','MB','GB'],i=0;while(n>=1024&&i<3){n/=1024;i++}return (i?n.toFixed(n<10?1:0):n)+' '+u[i]}
pick.addEventListener('click',function(){input.click()});
input.addEventListener('change',function(){add([].slice.call(input.files));input.value=''});
function add(files){
  if(!files.length)return;
  done.hidden=true;
  files.forEach(function(f){
    var li=document.createElement('li');li.className='file';
    var name=document.createElement('div');name.className='name';name.textContent=f.name;
    var st=document.createElement('div');st.className='st';st.textContent=sz(f.size)+' - waiting';
    var bar=document.createElement('div');bar.className='bar';var fill=document.createElement('i');bar.appendChild(fill);
    li.appendChild(name);li.appendChild(bar);li.appendChild(st);list.appendChild(li);
    var job={f:f,st:st,fill:fill,li:li};
    if(f.size>MAX){fail(job,'Too large (limit '+sz(MAX)+')');return}
    q.push(job);
  });
  if(!busy)next();
}
function fail(j,m){bad++;j.st.textContent=m;j.st.className='st bad';j.fill.style.width='0'}
function next(){
  var j=q.shift();
  if(!j){busy=false;finish();return}
  busy=true;j.st.textContent='Sending 0%';
  var x=new XMLHttpRequest();
  x.open('POST','/r/'+T+'/up?n='+encodeURIComponent(j.f.name));
  x.setRequestHeader('Content-Type','application/octet-stream');
  x.upload.onprogress=function(e){if(e.lengthComputable){var p=Math.round(e.loaded/e.total*100);j.fill.style.width=p+'%';j.st.textContent='Sending '+p+'%'}};
  x.onload=function(){
    if(x.status===200){ok++;bytes+=j.f.size;j.fill.style.width='100%';j.st.textContent='Sent - '+sz(j.f.size)}
    else if(x.status===404){fail(j,'This link has expired. Ask for a new QR code.')}
    else if(x.status===413){fail(j,'Too large for this link')}
    else{fail(j,'Could not save this file')}
    next();
  };
  x.onerror=function(){fail(j,'Lost connection. Are you still on the same Wi-Fi?');next()};
  x.send(j.f);
}
function finish(){
  if(ok>0){sum.textContent=ok+(ok===1?' file':' files')+' - '+sz(bytes)+(bad?' ('+bad+' failed)':'');done.hidden=false;pick.querySelector('span').textContent='Send more files'}
}
})();</script>"#;

/// PIN entry. Submits with GET so that POST stays reserved for uploads.
pub fn pin_page(action_path: &str, error: Option<&str>) -> String {
    let mut h = head("Krovi · Enter PIN");
    h.push_str(&format!(
        "<body><main>{}<h1>Enter the PIN</h1><p class=\"sub\">The 4-digit PIN is shown on the computer.</p>",
        brand()
    ));
    if let Some(e) = error {
        h.push_str(&format!("<p class=\"err\">{}</p>", html_escape(e)));
    }
    h.push_str(&format!(
        "<form method=\"get\" action=\"{}\" autocomplete=\"off\"><input class=\"pin\" name=\"pin\" inputmode=\"numeric\" pattern=\"[0-9]{{4}}\" maxlength=\"4\" autocomplete=\"off\" autofocus required aria-label=\"PIN\">\
<button class=\"btn\" type=\"submit\">Continue</button></form></main></body></html>",
        html_escape(action_path)
    ));
    h
}

/// The one error body for every unknown path, wrong token, expired share or wrong method.
pub const NOT_FOUND_BODY: &str = "Not found";

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, u64)> {
        vec![
            ("Report <final>.pdf".into(), 1_500_000),
            ("photo.jpg".into(), 300),
        ]
    }

    #[test]
    fn send_page_escapes_names_and_has_a_button_per_file() {
        let p = send_page("TOKEN", &files(), 540);
        assert!(p.contains("Report &lt;final&gt;.pdf"));
        assert!(!p.contains("Report <final>"));
        assert!(p.contains("href=\"/s/TOKEN/0\""));
        assert!(p.contains("href=\"/s/TOKEN/1\""));
        assert_eq!(p.matches("class=\"btn dl\"").count(), 2);
        assert!(p.contains("Download all (2)"));
        assert!(p.contains("2 files ready"));
        assert!(p.contains("1.4 MB"));
    }

    #[test]
    fn single_file_has_no_download_all() {
        let p = send_page("TOKEN", &files()[..1], 600);
        assert!(!p.contains("Download all"));
        assert!(p.contains("1 file ready"));
    }

    #[test]
    fn pages_are_self_contained() {
        for p in [
            send_page("T", &files(), 60),
            receive_page("T", 1 << 31, 60),
            pin_page("/s/T", None),
        ] {
            assert!(!p.contains("http://"), "no absolute http URLs");
            assert!(!p.contains("https://"), "no absolute https URLs");
            assert!(!p.contains("src=\"http"), "no remote scripts/images");
            assert!(!p.contains("@import"));
            assert!(!p.contains("<link"));
            assert!(p.contains("viewport"));
        }
    }

    #[test]
    fn receive_page_embeds_limit_and_token_only() {
        let p = receive_page("TOKEN", 2_147_483_648, 600);
        assert!(p.contains("data-token=\"TOKEN\""));
        assert!(p.contains("data-max=\"2147483648\""));
        assert!(p.contains("2.0 GB"));
        assert!(p.contains("/up?n="));
    }

    #[test]
    fn pin_page_uses_get_and_escapes_the_error() {
        let p = pin_page("/s/T", Some("Wrong <PIN>"));
        assert!(p.contains("method=\"get\""));
        assert!(!p.to_lowercase().contains("method=\"post\""));
        assert!(p.contains("Wrong &lt;PIN&gt;"));
    }

    #[test]
    fn byte_and_time_formatting() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
        assert_eq!(format_bytes(250 * 1024 * 1024), "250 MB");
        assert_eq!(expiry_text(30), "less than a minute");
        assert_eq!(expiry_text(61), "2 min");
        assert_eq!(expiry_text(600), "10 min");
        assert_eq!(expiry_text(3700), "1 h 1 min");
    }
}

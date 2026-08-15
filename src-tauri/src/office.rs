use calamine::{open_workbook_auto, Data, Reader};
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

const MAX_ROWS: usize = 400;
const MAX_COLS: usize = 40;
const MAX_PAGES: usize = 80;
const MAX_CHARS: usize = 200_000;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfficePreview {
    pub kind: String,
    pub sheets: Vec<OfficeSheet>,
    pub pages: Vec<OfficePage>,
    pub truncated: bool,
    pub note: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfficeSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfficePage {
    pub title: String,
    pub body: String,
}

pub fn preview(path: String) -> Result<OfficePreview, String> {
    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "csv" | "tsv" => preview_csv(&path, if ext == "tsv" { '\t' } else { ',' }),
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => preview_sheet(&path),
        "docx" | "odt" => preview_word_zip(&path, &ext),
        "pptx" | "odp" => preview_slides_zip(&path, &ext),
        "doc" | "rtf" | "rtfd" => preview_via_textutil(&path, "document"),
        "ppt" => preview_via_textutil(&path, "slides"),
        _ => preview_via_textutil(&path, "document"),
    }
}

fn preview_csv(path: &str, sep: char) -> Result<OfficePreview, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut truncated = false;
    for line in raw.lines() {
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        rows.push(split_csv(line, sep));
    }
    Ok(OfficePreview {
        kind: "spreadsheet".into(),
        sheets: vec![OfficeSheet {
            name: "Sheet".into(),
            rows,
        }],
        pages: vec![],
        truncated,
        note: String::new(),
    })
}

fn split_csv(line: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if quoted && chars.peek() == Some(&'"') {
                chars.next();
                cur.push('"');
            } else {
                quoted = !quoted;
            }
        } else if c == sep && !quoted {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        if out.len() >= MAX_COLS {
            break;
        }
    }
    out.push(cur);
    out.truncate(MAX_COLS);
    out
}

fn preview_sheet(path: &str) -> Result<OfficePreview, String> {
    let mut wb = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let names = wb.sheet_names().to_vec();
    if names.is_empty() {
        return Err("This workbook has no sheets".into());
    }
    let mut sheets = Vec::new();
    let mut truncated = false;
    for name in names.iter().take(12) {
        let Ok(range) = wb.worksheet_range(name) else {
            continue;
        };
        let mut rows = Vec::new();
        for (i, row) in range.rows().enumerate() {
            if i >= MAX_ROWS {
                truncated = true;
                break;
            }
            rows.push(
                row.iter()
                    .take(MAX_COLS)
                    .map(cell_text)
                    .collect::<Vec<_>>(),
            );
        }
        sheets.push(OfficeSheet {
            name: name.clone(),
            rows,
        });
    }
    Ok(OfficePreview {
        kind: "spreadsheet".into(),
        sheets,
        pages: vec![],
        truncated,
        note: String::new(),
    })
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(n) => trim_float(*n),
        Data::Int(n) => n.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("{e}"),
    }
}

fn trim_float(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n}")
    }
}

fn preview_word_zip(path: &str, ext: &str) -> Result<OfficePreview, String> {
    let inner = if ext == "odt" {
        "content.xml"
    } else {
        "word/document.xml"
    };
    let xml = zip_entry(path, inner)?;
    let body = clip(&xml_plain(&xml));
    if body.trim().is_empty() {
        return preview_via_textutil(path, "document");
    }
    Ok(OfficePreview {
        kind: "document".into(),
        sheets: vec![],
        pages: vec![OfficePage {
            title: Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Document".into()),
            body,
        }],
        truncated: xml.len() > MAX_CHARS,
        note: String::new(),
    })
}

fn preview_slides_zip(path: &str, ext: &str) -> Result<OfficePreview, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let item = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = item.name().to_string();
        if ext == "odp" {
            if name == "content.xml" {
                names.push(name);
            }
        } else if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            names.push(name);
        }
    }
    names.sort();
    if names.is_empty() {
        return preview_via_textutil(path, "slides");
    }
    let mut pages = Vec::new();
    for (i, name) in names.iter().take(MAX_PAGES).enumerate() {
        let xml = zip_read(&mut zip, name)?;
        pages.push(OfficePage {
            title: format!("Slide {}", i + 1),
            body: clip(&xml_plain(&xml)),
        });
    }
    Ok(OfficePreview {
        kind: "slides".into(),
        sheets: vec![],
        pages,
        truncated: names.len() > MAX_PAGES,
        note: String::new(),
    })
}

fn zip_entry(path: &str, name: &str) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    zip_read(&mut zip, name)
}

fn zip_read(zip: &mut ZipArchive<fs::File>, name: &str) -> Result<String, String> {
    let mut item = zip.by_name(name).map_err(|e| e.to_string())?;
    let mut buf = String::new();
    item.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn xml_plain(xml: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let bytes = xml.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '<' {
            let rest = &xml[i..];
            if rest.starts_with("</w:p")
                || rest.starts_with("</text:p")
                || rest.starts_with("</a:p")
                || rest.starts_with("<w:br")
                || rest.starts_with("<text:line-break")
            {
                out.push('\n');
            }
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
        i += 1;
    }
    decode_entities(&out)
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
}

fn clip(s: &str) -> String {
    if s.len() <= MAX_CHARS {
        s.to_string()
    } else {
        format!("{}…", &s[..MAX_CHARS])
    }
}

fn preview_via_textutil(path: &str, kind: &str) -> Result<OfficePreview, String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("textutil")
            .args(["-convert", "txt", "-stdout", path])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            let body = clip(&String::from_utf8_lossy(&out.stdout));
            if !body.trim().is_empty() {
                return Ok(OfficePreview {
                    kind: kind.into(),
                    sheets: vec![],
                    pages: vec![OfficePage {
                        title: Path::new(path)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "Document".into()),
                        body,
                    }],
                    truncated: false,
                    note: String::new(),
                });
            }
        }
    }
    Ok(OfficePreview {
        kind: kind.into(),
        sheets: vec![],
        pages: vec![],
        truncated: false,
        note: "Depot can list this file, but the preview needs Word, Pages, LibreOffice or Excel. Use Open with.".into(),
    })
}

use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use crate::error::{AppError, Result};

const BASE: &str = "https://api.notion.com/v1";
const VER:  &str = "2022-06-28";

// ── Client ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct NotionClient {
    http: Client,
    key: String,
    pub db_id: String,
}

impl NotionClient {
    pub fn new(key: impl Into<String>, db_id: impl Into<String>) -> Self {
        NotionClient { http: Client::new(), key: key.into(), db_id: db_id.into() }
    }

    fn auth(&self) -> String { format!("Bearer {}", self.key) }

    async fn get(&self, path: &str) -> Result<Value> {
        let r = self.http.get(format!("{BASE}{path}"))
            .header("Authorization", self.auth())
            .header("Notion-Version", VER)
            .send().await?;
        if !r.status().is_success() {
            return Err(AppError::Notion(r.text().await.unwrap_or_default()));
        }
        Ok(r.json().await?)
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let r = self.http.post(format!("{BASE}{path}"))
            .header("Authorization", self.auth())
            .header("Notion-Version", VER)
            .json(body).send().await?;
        if !r.status().is_success() {
            return Err(AppError::Notion(r.text().await.unwrap_or_default()));
        }
        Ok(r.json().await?)
    }

    async fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        let r = self.http.patch(format!("{BASE}{path}"))
            .header("Authorization", self.auth())
            .header("Notion-Version", VER)
            .json(body).send().await?;
        if !r.status().is_success() {
            return Err(AppError::Notion(r.text().await.unwrap_or_default()));
        }
        Ok(r.json().await?)
    }
}

// ── Database discovery (for the Database tab's "switch database" picker) ──────

/// A database the Notion integration can see, as returned by /v1/search.
#[derive(Debug, Clone)]
pub struct DbRef {
    pub id: String,
    pub title: String,
}

impl NotionClient {
    /// List databases this integration has access to (Notion `/v1/search`,
    /// filtered to objects of type "database"). Used by the Database tab's
    /// "switch database" picker (`D` key).
    pub async fn list_databases(&self) -> Result<Vec<DbRef>> {
        let body = json!({
            "filter": {"property": "object", "value": "database"},
            "page_size": 100
        });
        let v = self.post("/search", &body).await?;
        let dbs = v["results"].as_array().unwrap_or(&vec![]).iter().map(|r| {
            let id = r["id"].as_str().unwrap_or("").to_string();
            let title = r["title"].as_array()
                .and_then(|a| a.first())
                .and_then(|t| t["plain_text"].as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("(untitled database)")
                .to_string();
            DbRef { id, title }
        }).collect();
        Ok(dbs)
    }
}

// ── Schema ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct PropDef {
    pub name: String,
    pub kind: String,            // title | rich_text | select | checkbox | number | email | …
    pub options: Vec<String>,    // for select / multi_select / status
    pub editable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Schema {
    pub props: Vec<PropDef>,     // ordered: title first, then alpha
}

impl Schema {
    pub fn editable(&self) -> impl Iterator<Item = &PropDef> {
        self.props.iter().filter(|p| p.editable)
    }
}

impl NotionClient {
    /// Fetch the schema for this client's bound database (used by Members).
    pub async fn fetch_schema(&self) -> Result<Schema> {
        self.fetch_schema_of(&self.db_id).await
    }

    /// Fetch the schema for an arbitrary database id. Used by the Database
    /// tab, which can point at a different database than `self.db_id`.
    pub async fn fetch_schema_of(&self, db_id: &str) -> Result<Schema> {
        let v = self.get(&format!("/databases/{db_id}")).await?;
        Ok(parse_schema(&v))
    }
}

fn parse_schema(v: &Value) -> Schema {
    let mut props = vec![];
    if let Some(obj) = v["properties"].as_object() {
        for (name, def) in obj {
            let kind = def["type"].as_str().unwrap_or("").to_string();
            let editable = !matches!(kind.as_str(),
                "formula"|"rollup"|"created_time"|"last_edited_time"|"created_by"|"last_edited_by");
            let mut options = vec![];
            for arr_key in &["select","multi_select","status"] {
                if let Some(opts) = def[arr_key]["options"].as_array() {
                    options = opts.iter().filter_map(|o| o["name"].as_str()).map(String::from).collect();
                    break;
                }
            }
            props.push(PropDef { name: name.clone(), kind, options, editable });
        }
    }
    props.sort_by(|a, b| {
        if a.kind == "title" { std::cmp::Ordering::Less }
        else if b.kind == "title" { std::cmp::Ordering::Greater }
        else { a.name.cmp(&b.name) }
    });
    Schema { props }
}

// ── Page ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Page {
    pub id: String,
    pub props: HashMap<String, Value>,
}

impl Page {
    /// Extract a human-readable string for a property value.
    pub fn display(&self, key: &str) -> String {
        match self.props.get(key) {
            Some(v) => extract_value(v),
            None => String::new(),
        }
    }
}

pub fn extract_value(v: &Value) -> String {
    match v["type"].as_str().unwrap_or("") {
        "title"        => rt_text(&v["title"]),
        "rich_text"    => rt_text(&v["rich_text"]),
        "select"       => v["select"]["name"].as_str().unwrap_or("").into(),
        "status"       => v["status"]["name"].as_str().unwrap_or("").into(),
        "multi_select" => v["multi_select"].as_array()
            .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
        "checkbox"     => if v["checkbox"].as_bool().unwrap_or(false) { "✓" } else { "✗" }.into(),
        "number"       => v["number"].as_f64()
            .map(|n| if n.fract() == 0.0 { (n as i64).to_string() } else { format!("{n:.2}") })
            .unwrap_or_default(),
        "email"        => v["email"].as_str().unwrap_or("").into(),
        "url"          => v["url"].as_str().unwrap_or("").into(),
        "phone_number" => v["phone_number"].as_str().unwrap_or("").into(),
        "date"         => v["date"]["start"].as_str().unwrap_or("").into(),
        "people"       => v["people"].as_array()
            .map(|a| a.iter().filter_map(|p| p["name"].as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn rt_text(arr: &Value) -> String {
    arr.as_array()
        .and_then(|a| a.first())
        .and_then(|x| x["text"]["content"].as_str())
        .unwrap_or("")
        .into()
}

/// Build a Notion property payload from a plain string value.
pub fn build_prop(kind: &str, value: &str) -> Value {
    match kind {
        "title"        => json!({"title":     [{"text":{"content": value}}]}),
        "rich_text"    => json!({"rich_text": [{"text":{"content": value}}]}),
        "select"       => if value.is_empty() { json!({"select": null}) } else { json!({"select":{"name":value}}) },
        "status"       => json!({"status":{"name":value}}),
        "multi_select" => {
            let names: Vec<Value> = value.split(',').map(|s| s.trim())
                .filter(|s| !s.is_empty()).map(|s| json!({"name":s})).collect();
            json!({"multi_select": names})
        },
        "checkbox"     => json!({"checkbox": matches!(value, "true"|"yes"|"1"|"✓")}),
        "number"       => value.parse::<f64>()
            .map(|n| json!({"number": n}))
            .unwrap_or(json!({"number": null})),
        "email"        => json!({"email": value}),
        "url"          => json!({"url": value}),
        "phone_number" => json!({"phone_number": value}),
        "date"         => if value.is_empty() { json!({"date":null}) } else { json!({"date":{"start":value}}) },
        _ => json!({}),
    }
}

// ── CRUD ─────────────────────────────────────────────────────────────────────

impl NotionClient {
    /// Query this client's bound database. Pass optional Notion filter JSON.
    pub async fn query(&self, filter: Option<Value>) -> Result<Vec<Page>> {
        self.query_of(&self.db_id, filter).await
    }

    /// Query an arbitrary database id. Used by the Database tab.
    pub async fn query_of(&self, db_id: &str, filter: Option<Value>) -> Result<Vec<Page>> {
        let mut body = json!({"page_size": 100});
        if let Some(f) = filter { body["filter"] = f; }
        let v = self.post(&format!("/databases/{db_id}/query"), &body).await?;
        Ok(v["results"].as_array().unwrap_or(&vec![]).iter().map(parse_page).collect())
    }

    /// Full-text search across this client's bound database.
    pub async fn search_title(&self, text: &str) -> Result<Vec<Page>> {
        self.search_title_of(&self.db_id, text).await
    }

    /// Full-text search across an arbitrary database id. Used by the
    /// Database tab. Assumes the title property is named "Name"; if a
    /// database uses a different title property name this will return no
    /// matches (use the unfiltered `query_of` + client-side filtering for
    /// databases with non-standard title property names).
    pub async fn search_title_of(&self, db_id: &str, text: &str) -> Result<Vec<Page>> {
        let filter = json!({"property":"Name","title":{"contains":text}});
        self.query_of(db_id, Some(filter)).await
    }

    pub async fn create_page(&self, props: HashMap<String, Value>) -> Result<Page> {
        self.create_page_in(&self.db_id, props).await
    }

    /// Create a page in an arbitrary database id. Used by the Database tab.
    pub async fn create_page_in(&self, db_id: &str, props: HashMap<String, Value>) -> Result<Page> {
        let body = json!({"parent":{"database_id":db_id},"properties":props});
        let v = self.post("/pages", &body).await?;
        Ok(parse_page(&v))
    }

    pub async fn update_page(&self, id: &str, props: HashMap<String, Value>) -> Result<Page> {
        let body = json!({"properties": props});
        let v = self.patch(&format!("/pages/{id}"), &body).await?;
        Ok(parse_page(&v))
    }

    pub async fn archive_page(&self, id: &str) -> Result<()> {
        self.patch(&format!("/pages/{id}"), &json!({"archived":true})).await?;
        Ok(())
    }

    pub async fn unarchive_page(&self, id: &str) -> Result<()> {
        self.patch(&format!("/pages/{id}"), &json!({"archived":false})).await?;
        Ok(())
    }
}

fn parse_page(v: &Value) -> Page {
    let props = v["properties"].as_object()
        .map(|o| o.iter().map(|(k, val)| (k.clone(), val.clone())).collect())
        .unwrap_or_default();
    Page { id: v["id"].as_str().unwrap_or("").into(), props }
}

//! GitHub client: Projects V2 (GraphQL) + issues / comments / PRs (REST).

use crate::db::SyncedItem;
use crate::model::Settings;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct Github {
    http: reqwest::Client,
    token: String,
    api: String,
}

/// API root. Override with MAESTRO_GITHUB_API for GitHub Enterprise
/// (e.g. https://ghe.example.com/api/v3) — GraphQL is derived from it.
pub fn api_base() -> String {
    std::env::var("MAESTRO_GITHUB_API").unwrap_or_else(|_| "https://api.github.com".into()).trim_end_matches('/').to_string()
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    pub status_field_id: String,
    /// (option id, option name)
    pub options: Vec<(String, String)>,
}

impl ProjectInfo {
    pub fn option_id(&self, name: &str) -> Option<String> {
        self.options.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(i, _)| i.clone())
    }
}

/// Resolve the GitHub token from settings (explicit token or `gh auth token`).
pub async fn resolve_token(s: &Settings) -> Result<String> {
    if s.github_auth == "gh_cli" {
        let out = tokio::process::Command::new("gh").args(["auth", "token"]).output().await
            .context("`gh` CLI not found — install it or use a personal access token")?;
        if !out.status.success() {
            bail!("`gh auth token` failed — run `gh auth login` first");
        }
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    if s.github_token.trim().is_empty() {
        bail!("No GitHub token configured (Settings → GitHub)");
    }
    Ok(s.github_token.trim().to_string())
}

impl Github {
    pub fn new(token: String) -> Github {
        let http = reqwest::Client::builder()
            .user_agent("maestro-orchestrator")
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();
        Github { http, token, api: api_base() }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    async fn graphql(&self, query: &str, vars: Value) -> Result<Value> {
        let res: Value = self
            .http
            .post(if self.api.ends_with("/api/v3") { format!("{}/graphql", self.api.trim_end_matches("/v3")) } else { format!("{}/graphql", self.api) })
            .bearer_auth(&self.token)
            .json(&json!({"query": query, "variables": vars}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(errs) = res.get("errors").and_then(|e| e.as_array()) {
            if !errs.is_empty() && res.get("data").map(|d| d.is_null()).unwrap_or(true) {
                bail!("GitHub GraphQL: {}", errs[0]["message"].as_str().unwrap_or("error"));
            }
        }
        Ok(res["data"].clone())
    }

    async fn rest(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.api))
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("GitHub {path} → {status}: {}", text.chars().take(400).collect::<String>());
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn viewer(&self) -> Result<String> {
        let d = self.graphql("query{viewer{login}}", json!({})).await?;
        Ok(d["viewer"]["login"].as_str().unwrap_or_default().to_string())
    }

    pub async fn project(&self, owner: &str, number: i64, status_field: &str) -> Result<ProjectInfo> {
        const FIELDS: &str = "id title url fields(first:50){nodes{... on ProjectV2SingleSelectField{id name options{id name}}}}";
        let mut proj = Value::Null;
        for kind in ["organization", "user"] {
            let q = format!("query($l:String!,$n:Int!){{ {kind}(login:$l){{ projectV2(number:$n){{ {FIELDS} }} }} }}");
            if let Ok(d) = self.graphql(&q, json!({"l": owner, "n": number})).await {
                if !d[kind]["projectV2"].is_null() {
                    proj = d[kind]["projectV2"].clone();
                    break;
                }
            }
        }
        if proj.is_null() {
            bail!("Project #{number} not found for '{owner}' (check owner, number and token scopes: project, repo)");
        }
        let field = proj["fields"]["nodes"]
            .as_array()
            .and_then(|a| a.iter().find(|f| f["name"].as_str().map(|n| n.eq_ignore_ascii_case(status_field)).unwrap_or(false)))
            .ok_or_else(|| anyhow!("Single-select field '{status_field}' not found in project"))?;
        Ok(ProjectInfo {
            id: proj["id"].as_str().unwrap_or_default().into(),
            title: proj["title"].as_str().unwrap_or_default().into(),
            url: proj["url"].as_str().unwrap_or_default().into(),
            status_field_id: field["id"].as_str().unwrap_or_default().into(),
            options: field["options"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|o| (o["id"].as_str().unwrap_or_default().into(), o["name"].as_str().unwrap_or_default().into()))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    pub async fn items(&self, project_id: &str, status_field: &str) -> Result<Vec<SyncedItem>> {
        const Q: &str = r#"query($p:ID!,$c:String,$f:String!){ node(id:$p){ ... on ProjectV2 { items(first:100, after:$c){
            pageInfo{hasNextPage endCursor}
            nodes{ id isArchived
              fieldValueByName(name:$f){ ... on ProjectV2ItemFieldSingleSelectValue{ name } }
              content{ __typename
                ... on Issue{ id number title body state url labels(first:30){nodes{name}} repository{nameWithOwner} }
              }
            } } } } }"#;
        let mut out = vec![];
        let mut cursor: Option<String> = None;
        loop {
            let d = self.graphql(Q, json!({"p": project_id, "c": cursor, "f": status_field})).await?;
            let items = &d["node"]["items"];
            for n in items["nodes"].as_array().cloned().unwrap_or_default() {
                let c = &n["content"];
                if c["__typename"] != "Issue" || n["isArchived"].as_bool().unwrap_or(false) {
                    continue; // draft issues / PRs are not actionable
                }
                out.push(SyncedItem {
                    item_id: n["id"].as_str().unwrap_or_default().into(),
                    issue_node_id: c["id"].as_str().unwrap_or_default().into(),
                    repo: c["repository"]["nameWithOwner"].as_str().unwrap_or_default().into(),
                    number: c["number"].as_i64().unwrap_or(0),
                    title: c["title"].as_str().unwrap_or_default().into(),
                    body: c["body"].as_str().unwrap_or_default().into(),
                    url: c["url"].as_str().unwrap_or_default().into(),
                    labels: c["labels"]["nodes"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|l| l["name"].as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                    gh_status: n["fieldValueByName"]["name"].as_str().unwrap_or_default().into(),
                    gh_state: c["state"].as_str().unwrap_or("OPEN").into(),
                });
            }
            if items["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false) {
                cursor = items["pageInfo"]["endCursor"].as_str().map(String::from);
            } else {
                break;
            }
        }
        Ok(out)
    }

    pub async fn set_status(&self, p: &ProjectInfo, item_id: &str, column: &str) -> Result<()> {
        if column.trim().is_empty() {
            return Ok(());
        }
        let opt = p.option_id(column).ok_or_else(|| anyhow!("Status column '{column}' does not exist in the project"))?;
        const M: &str = "mutation($p:ID!,$i:ID!,$f:ID!,$o:String!){ updateProjectV2ItemFieldValue(input:{projectId:$p,itemId:$i,fieldId:$f,value:{singleSelectOptionId:$o}}){ projectV2Item{id} } }";
        self.graphql(M, json!({"p": p.id, "i": item_id, "f": p.status_field_id, "o": opt})).await?;
        Ok(())
    }

    /// Create an issue and add it to the project. Returns (number, url, item_id).
    pub async fn create_issue(&self, p: &ProjectInfo, repo: &str, title: &str, body: &str, labels: &[String]) -> Result<(i64, String, String)> {
        let mut payload = json!({"title": title, "body": body});
        if !labels.is_empty() {
            payload["labels"] = json!(labels);
        }
        let issue = match self.rest(reqwest::Method::POST, &format!("/repos/{repo}/issues"), Some(payload.clone())).await {
            Ok(v) => v,
            Err(_) if !labels.is_empty() => {
                // Labels may not exist / no permission: retry without them.
                payload.as_object_mut().unwrap().remove("labels");
                self.rest(reqwest::Method::POST, &format!("/repos/{repo}/issues"), Some(payload)).await?
            }
            Err(e) => return Err(e),
        };
        let node = issue["node_id"].as_str().unwrap_or_default();
        const M: &str = "mutation($p:ID!,$c:ID!){ addProjectV2ItemById(input:{projectId:$p,contentId:$c}){ item{id} } }";
        let d = self.graphql(M, json!({"p": p.id, "c": node})).await?;
        Ok((
            issue["number"].as_i64().unwrap_or(0),
            issue["html_url"].as_str().unwrap_or_default().into(),
            d["addProjectV2ItemById"]["item"]["id"].as_str().unwrap_or_default().into(),
        ))
    }

    pub async fn comment(&self, repo: &str, number: i64, body: &str) -> Result<()> {
        self.rest(reqwest::Method::POST, &format!("/repos/{repo}/issues/{number}/comments"), Some(json!({"body": body}))).await?;
        Ok(())
    }

    pub async fn issue_comments(&self, repo: &str, number: i64) -> Result<Vec<(String, String)>> {
        let v = self.rest(reqwest::Method::GET, &format!("/repos/{repo}/issues/{number}/comments?per_page=50"), None).await?;
        Ok(v.as_array()
            .map(|a| a.iter().map(|c| (c["user"]["login"].as_str().unwrap_or("?").into(), c["body"].as_str().unwrap_or("").into())).collect())
            .unwrap_or_default())
    }

    pub async fn default_branch(&self, repo: &str) -> Result<String> {
        let v = self.rest(reqwest::Method::GET, &format!("/repos/{repo}"), None).await?;
        Ok(v["default_branch"].as_str().unwrap_or("main").to_string())
    }

    /// Open a PR, or return the existing one for that branch. Returns (number, url).
    pub async fn open_pr(&self, repo: &str, head: &str, base: &str, title: &str, body: &str) -> Result<(i64, String)> {
        let owner = repo.split('/').next().unwrap_or_default();
        let existing = self
            .rest(reqwest::Method::GET, &format!("/repos/{repo}/pulls?state=open&head={owner}:{head}"), None)
            .await?;
        if let Some(pr) = existing.as_array().and_then(|a| a.first()) {
            return Ok((pr["number"].as_i64().unwrap_or(0), pr["html_url"].as_str().unwrap_or_default().into()));
        }
        let pr = self
            .rest(reqwest::Method::POST, &format!("/repos/{repo}/pulls"), Some(json!({"title": title, "head": head, "base": base, "body": body})))
            .await?;
        Ok((pr["number"].as_i64().unwrap_or(0), pr["html_url"].as_str().unwrap_or_default().into()))
    }

    /// "open" | "merged" | "closed"
    pub async fn pr_state(&self, repo: &str, number: i64) -> Result<String> {
        let v = self.rest(reqwest::Method::GET, &format!("/repos/{repo}/pulls/{number}"), None).await?;
        Ok(if v["merged"].as_bool().unwrap_or(false) {
            "merged".into()
        } else {
            v["state"].as_str().unwrap_or("open").into()
        })
    }

    pub async fn merge_pr(&self, repo: &str, number: i64) -> Result<()> {
        self.rest(reqwest::Method::PUT, &format!("/repos/{repo}/pulls/{number}/merge"), Some(json!({"merge_method": "squash"}))).await?;
        Ok(())
    }

    pub async fn review_comment(&self, repo: &str, pr: i64, body: &str) -> Result<()> {
        self.comment(repo, pr, body).await
    }
}

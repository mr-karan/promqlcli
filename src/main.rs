use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use reqwest::blocking::{Client, RequestBuilder};
use serde::Deserialize;
use serde_json::Value;
use url::Url;

#[derive(Parser)]
#[command(
    name = "prometheus-metrics",
    version,
    about = "Query Prometheus/VictoriaMetrics endpoints"
)]
struct Cli {
    #[arg(long, env = "PROMQL_BASE_URL", value_name = "URL")]
    base_url: String,

    /// Basic auth in the form user:password
    #[arg(long, env = "PROMQL_AUTH")]
    auth: Option<String>,

    #[arg(long, env = "PROMQL_USER")]
    user: Option<String>,

    #[arg(long, env = "PROMQL_PASS")]
    password: Option<String>,

    /// Bearer token (overrides basic auth)
    #[arg(long, env = "PROMQL_BEARER")]
    bearer: Option<String>,

    /// Pretty-print JSON output
    #[arg(long, default_value_t = false)]
    pretty: bool,

    /// Print only .data.result when available
    #[arg(long, default_value_t = false)]
    result: bool,

    /// Print list endpoints as one value per line
    #[arg(long, default_value_t = false)]
    lines: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Instant query
    Query {
        /// PromQL query
        query: String,
        /// Evaluation timestamp (RFC3339 or Unix timestamp)
        #[arg(long)]
        time: Option<String>,
        /// Optional query timeout (e.g. 30s)
        #[arg(long)]
        timeout: Option<String>,
    },

    /// Range query
    Range {
        /// PromQL query
        query: String,
        /// Range start (RFC3339 or Unix timestamp)
        #[arg(long)]
        start: String,
        /// Range end (RFC3339 or Unix timestamp)
        #[arg(long)]
        end: String,
        /// Step size (e.g. 60s)
        #[arg(long, default_value = "60s")]
        step: String,
        /// Optional query timeout (e.g. 30s)
        #[arg(long)]
        timeout: Option<String>,
    },

    /// List label values
    Labels {
        /// Label name
        label: String,
        /// Matchers to filter label values (repeatable)
        #[arg(long = "match")]
        matches: Vec<String>,
    },

    /// List job label values
    Jobs,

    /// List metric names
    Metrics {
        /// Case-insensitive substring filter
        #[arg(long)]
        filter: Option<String>,
    },

    /// Find series matching selector(s)
    Series {
        /// Matchers to filter series (repeatable)
        #[arg(long = "match")]
        matches: Vec<String>,
        /// Range start (RFC3339 or Unix timestamp)
        #[arg(long)]
        start: Option<String>,
        /// Range end (RFC3339 or Unix timestamp)
        #[arg(long)]
        end: Option<String>,
    },
}

#[derive(Deserialize)]
struct ApiResponse {
    status: String,
    data: Option<Value>,
    #[serde(rename = "errorType")]
    error_type: Option<String>,
    error: Option<String>,
    warnings: Option<Vec<String>>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let base = normalize_base(&cli.base_url)?;
    let client = Client::builder()
        .user_agent(format!("prometheus-metrics/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")?;

    match &cli.command {
        Commands::Query {
            query,
            time,
            timeout,
        } => {
            let url = base.join("api/v1/query").context("invalid base URL")?;
            let mut params = vec![("query".to_string(), query.clone())];
            if let Some(time) = time {
                params.push(("time".to_string(), time.clone()));
            }
            if let Some(timeout) = timeout {
                params.push(("timeout".to_string(), timeout.clone()));
            }
            let response = post_form(&cli, &client, url, params)?;
            output_data(&cli, response)?;
        }

        Commands::Range {
            query,
            start,
            end,
            step,
            timeout,
        } => {
            let url = base
                .join("api/v1/query_range")
                .context("invalid base URL")?;
            let mut params = vec![
                ("query".to_string(), query.clone()),
                ("start".to_string(), start.clone()),
                ("end".to_string(), end.clone()),
                ("step".to_string(), step.clone()),
            ];
            if let Some(timeout) = timeout {
                params.push(("timeout".to_string(), timeout.clone()));
            }
            let response = post_form(&cli, &client, url, params)?;
            output_data(&cli, response)?;
        }

        Commands::Labels { label, matches } => {
            let url = base
                .join(&format!("api/v1/label/{label}/values"))
                .context("invalid base URL")?;
            let params = build_match_params(matches.clone(), None, None);
            let response = get_query(&cli, &client, url, params)?;
            output_list(&cli, response)?;
        }

        Commands::Jobs => {
            let url = base
                .join("api/v1/label/job/values")
                .context("invalid base URL")?;
            let response = get_query(&cli, &client, url, Vec::new())?;
            output_list(&cli, response)?;
        }

        Commands::Metrics { filter } => {
            let url = base
                .join("api/v1/label/__name__/values")
                .context("invalid base URL")?;
            let mut response = get_query(&cli, &client, url, Vec::new())?;
            if let Some(filter) = filter {
                response = filter_values(response, filter)?;
            }
            output_list(&cli, response)?;
        }

        Commands::Series {
            matches,
            start,
            end,
        } => {
            if matches.is_empty() {
                bail!("--match is required for series queries");
            }
            let url = base.join("api/v1/series").context("invalid base URL")?;
            let params = build_match_params(matches.clone(), start.clone(), end.clone());
            let response = get_query(&cli, &client, url, params)?;
            output_data(&cli, response)?;
        }
    }

    Ok(())
}

fn normalize_base(base: &str) -> Result<Url> {
    let mut base = base.to_string();
    if !base.ends_with('/') {
        base.push('/');
    }
    Url::parse(&base).context("invalid base URL")
}

fn apply_auth(request: RequestBuilder, cli: &Cli) -> Result<RequestBuilder> {
    if let Some(token) = &cli.bearer {
        return Ok(request.bearer_auth(token));
    }

    if let Some(auth) = &cli.auth {
        let (user, pass) = split_auth(auth)?;
        return Ok(request.basic_auth(user, Some(pass)));
    }

    if cli.user.is_some() || cli.password.is_some() {
        let user = cli
            .user
            .as_ref()
            .context("--user is required when using --password")?
            .to_string();
        let pass = cli
            .password
            .as_ref()
            .context("--password is required when using --user")?
            .to_string();
        return Ok(request.basic_auth(user, Some(pass)));
    }

    Ok(request)
}

fn split_auth(auth: &str) -> Result<(String, String)> {
    let mut parts = auth.splitn(2, ':');
    let user = parts.next().unwrap_or_default();
    let pass = parts.next();
    if user.is_empty() || pass.is_none() {
        bail!("auth must be in the form user:password");
    }
    Ok((user.to_string(), pass.unwrap().to_string()))
}

fn post_form(
    cli: &Cli,
    client: &Client,
    url: Url,
    params: Vec<(String, String)>,
) -> Result<ApiResponse> {
    let request = client.post(url).form(&params);
    let request = apply_auth(request, cli)?;
    let response = request.send().context("request failed")?;
    parse_response(response)
}

fn get_query(
    cli: &Cli,
    client: &Client,
    url: Url,
    params: Vec<(String, String)>,
) -> Result<ApiResponse> {
    let request = client.get(url).query(&params);
    let request = apply_auth(request, cli)?;
    let response = request.send().context("request failed")?;
    parse_response(response)
}

fn parse_response(response: reqwest::blocking::Response) -> Result<ApiResponse> {
    let status = response.status();
    let text = response.text().context("failed to read response body")?;
    let parsed: ApiResponse = serde_json::from_str(&text).with_context(|| {
        let preview = text.chars().take(200).collect::<String>();
        format!("failed to parse response as JSON (status {status}): {preview}")
    })?;

    if parsed.status != "success" {
        let error_type = parsed.error_type.unwrap_or_else(|| "unknown".to_string());
        let error = parsed.error.unwrap_or_else(|| "unknown error".to_string());
        bail!("API error ({error_type}): {error}");
    }

    if let Some(warnings) = &parsed.warnings {
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
    }

    Ok(parsed)
}

fn output_data(cli: &Cli, response: ApiResponse) -> Result<()> {
    let data = response.data.unwrap_or(Value::Null);
    let payload = if cli.result {
        data.get("result").cloned().unwrap_or(data)
    } else {
        data
    };
    print_json(&payload, cli.pretty)
}

fn output_list(cli: &Cli, response: ApiResponse) -> Result<()> {
    let data = response.data.unwrap_or(Value::Null);
    if cli.lines {
        print_lines(&data)
    } else {
        print_json(&data, cli.pretty)
    }
}

fn print_json(value: &Value, pretty: bool) -> Result<()> {
    let output = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    println!("{output}");
    Ok(())
}

fn print_lines(value: &Value) -> Result<()> {
    let Some(items) = value.as_array() else {
        bail!("expected an array response for lines output");
    };
    for item in items {
        if let Some(s) = item.as_str() {
            println!("{s}");
        } else {
            println!("{item}");
        }
    }
    Ok(())
}

fn build_match_params(
    matches: Vec<String>,
    start: Option<String>,
    end: Option<String>,
) -> Vec<(String, String)> {
    let mut params = Vec::new();
    for matcher in matches {
        params.push(("match[]".to_string(), matcher));
    }
    if let Some(start) = start {
        params.push(("start".to_string(), start));
    }
    if let Some(end) = end {
        params.push(("end".to_string(), end));
    }
    params
}

fn filter_values(response: ApiResponse, filter: &str) -> Result<ApiResponse> {
    let filter = filter.to_lowercase();
    let data = response.data.unwrap_or(Value::Null);
    let Some(items) = data.as_array() else {
        return Ok(ApiResponse {
            status: response.status,
            data: Some(data),
            error_type: response.error_type,
            error: response.error,
            warnings: response.warnings,
        });
    };

    let filtered: Vec<Value> = items
        .iter()
        .filter(|item| {
            item.as_str()
                .map(|s| s.to_lowercase().contains(&filter))
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    Ok(ApiResponse {
        status: response.status,
        data: Some(Value::Array(filtered)),
        error_type: response.error_type,
        error: response.error,
        warnings: response.warnings,
    })
}

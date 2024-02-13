use constants::{
    API_HOST, BASE_API, DATA_APP_ID, DATA_VERSION_CODE, HEADER_PIECE, LIB_VERSION, SJ_APP_ID,
    VALUE_PIECE, VM_APP_ID,
};
use futures_util::StreamExt;
use helper::generate_random_token;
use models::{
    AccountEntitlementsResponse, AccountLoginResponse, MangaAuthResponse, MangaChapterDetail,
    MangaDetail, MangaReadMetadataResponse, MangaSeriesResponse, MangaStoreInfo,
    MangaStoreResponse, MangaUrlResponse, SimpleResponse,
};
use std::collections::HashMap;
use tokio::io::{self, AsyncWriteExt};

pub mod config;
pub mod constants;
pub mod helper;
pub mod imaging;
pub mod models;

pub use config::*;

/// Main client for interacting with the VM API.
///
/// # Examples
/// ```no_run,ignore
/// use tosho_sjv::{SJClient, SJConfig, SJMode};
/// use tosho_sjv::constants::get_constants;
///
/// #[tokio::main]
/// async fn main() {
///     let config = SJConfig {
///         user_id: 123,
///         token: "xyz987abc",
///         instance: "abcxyz",
///     };
///     let constants = get_constants(1);
///
///     let client = SJClient::new(config, constants, SJMode::VM);
///     let manga = client.get_manga(777).await.unwrap();
///     println!("{:?}", manga);
/// }
/// ```
#[derive(Debug)]
pub struct SJClient {
    #[allow(dead_code)]
    inner: reqwest::Client,
    config: SJConfig,
    constants: &'static crate::constants::Constants,
    mode: SJMode,
}

impl SJClient {
    /// Create a new client instance.
    ///
    /// # Parameters
    /// * `config` - The configuration to use for the client.
    /// * `constants` - The constants to use for the client.
    /// * `mode` - The mode to use for the client.
    pub fn new(
        config: SJConfig,
        constants: &'static crate::constants::Constants,
        mode: SJMode,
    ) -> Self {
        Self::make_client(config, constants, mode, None)
    }

    /// Attach a proxy to the client.
    ///
    /// This will clone the client and return a new client with the proxy attached.
    ///
    /// # Arguments
    /// * `proxy` - The proxy to attach to the client
    pub fn with_proxy(&self, proxy: reqwest::Proxy) -> Self {
        Self::make_client(self.config.clone(), self.constants, self.mode, Some(proxy))
    }

    fn make_client(
        config: SJConfig,
        constants: &'static crate::constants::Constants,
        mode: SJMode,
        proxy: Option<reqwest::Proxy>,
    ) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(constants.ua),
        );
        headers.insert(
            reqwest::header::HOST,
            reqwest::header::HeaderValue::from_static(&API_HOST),
        );
        let referer = match mode {
            SJMode::VM => &constants.vm_name,
            SJMode::SJ => &constants.sj_name,
        };
        headers.insert(
            reqwest::header::REFERER,
            reqwest::header::HeaderValue::from_str(referer).unwrap(),
        );

        let x_header = format!("{} {}", constants.app_ver, *VALUE_PIECE);
        headers.insert(
            reqwest::header::HeaderName::from_static(&HEADER_PIECE),
            reqwest::header::HeaderValue::from_str(&x_header).unwrap(),
        );

        let client = reqwest::Client::builder().default_headers(headers);

        let client = match proxy {
            Some(proxy) => client.proxy(proxy).build().unwrap(),
            None => client.build().unwrap(),
        };

        Self {
            inner: client,
            config,
            constants,
            mode,
        }
    }

    /// Return the mode of the client.
    pub fn get_mode(&self) -> &SJMode {
        &self.mode
    }

    /// Make an authenticated request to the API.
    ///
    /// This request will automatically add all the required headers/cookies/auth method
    /// to the request.
    ///
    /// # Arguments
    /// * `method` - The HTTP method to use
    /// * `endpoint` - The endpoint to request (e.g. `/episode/list`)
    /// * `data` - The data to send in the request body (as form data)
    /// * `params` - The query params to send in the request
    async fn request<T>(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        data: Option<HashMap<String, String>>,
        params: Option<HashMap<String, String>>,
    ) -> anyhow::Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let endpoint = format!("{}{}", BASE_API.as_str(), endpoint);

        let request = match (data.clone(), params.clone()) {
            (None, None) => self.inner.request(method, endpoint),
            (Some(data), None) => {
                let mut extend_headers = reqwest::header::HeaderMap::new();
                extend_headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded".parse()?,
                );
                self.inner
                    .request(method, endpoint)
                    .form(&data)
                    .headers(extend_headers)
            }
            (None, Some(params)) => self.inner.request(method, endpoint).query(&params),
            (Some(_), Some(_)) => {
                anyhow::bail!("Cannot have both data and params")
            }
        };

        parse_response(request.send().await?).await
    }

    /// Get the manga store cache that can be use for other route.
    ///
    /// Can be used to get every possible manga series.
    pub async fn get_store_cache(&self) -> anyhow::Result<MangaStoreResponse> {
        let app_id = match self.mode {
            SJMode::VM => VM_APP_ID,
            SJMode::SJ => SJ_APP_ID,
        };
        let endpoint = format!(
            "/manga/store_cached/{}/{}/{}",
            app_id, self.constants.device_id, LIB_VERSION
        );

        let response = self
            .request::<MangaStoreResponse>(reqwest::Method::GET, &endpoint, None, None)
            .await?;

        Ok(response)
    }

    /// Get the list of manga from the given list of manga IDs
    ///
    /// # Arguments
    /// * `manga_ids` - The list of manga IDs to get
    pub async fn get_manga(&self, manga_ids: Vec<u32>) -> anyhow::Result<Vec<MangaDetail>> {
        let response = self.get_store_cache().await?;

        let manga_lists: Vec<MangaDetail> = response
            .contents
            .iter()
            .filter_map(|info| match info {
                MangaStoreInfo::Manga(manga) => Some(manga),
                _ => None,
            })
            .filter_map(|manga| {
                if manga_ids.contains(&manga.id) {
                    Some(manga.clone())
                } else {
                    None
                }
            })
            .collect();

        Ok(manga_lists)
    }

    /// Get list of chapters for specific manga ID
    ///
    /// # Arguments
    /// * `id` - The manga ID
    pub async fn get_chapters(&self, id: u32) -> anyhow::Result<Vec<MangaChapterDetail>> {
        let app_id = match self.mode {
            SJMode::VM => VM_APP_ID,
            SJMode::SJ => SJ_APP_ID,
        };
        let endpoint = format!(
            "/manga/store/series/{}/{}/{}/{}",
            id, app_id, self.constants.device_id, LIB_VERSION
        );

        let response = self
            .request::<MangaSeriesResponse>(reqwest::Method::GET, &endpoint, None, None)
            .await?;

        let mapped_chapters: Vec<MangaChapterDetail> = response
            .chapters
            .iter()
            .map(|data| data.chapter.clone())
            .collect();

        Ok(mapped_chapters)
    }

    /// Check if specific chapter can be viewed by us.
    ///
    /// # Arguments
    /// * `id` - The chapter ID
    pub async fn verify_chapter(&self, id: u32) -> anyhow::Result<()> {
        let mut data = common_data_hashmap(self.constants, &self.mode, Some(&self.config));
        data.insert("manga_id".to_string(), id.to_string());

        self.request::<MangaAuthResponse>(reqwest::Method::POST, "/manga/auth", Some(data), None)
            .await?;

        Ok(())
    }

    /// Get manga URL for specific chapter ID
    ///
    /// # Arguments
    /// * `id` - The chapter ID
    /// * `metadata` - Fetch metadata
    /// * `page` - Fetch specific page
    ///
    /// Metadata fetch will take precedent
    pub async fn get_manga_url(
        &self,
        id: u32,
        metadata: bool,
        page: Option<u32>,
    ) -> anyhow::Result<String> {
        let mut data = common_data_hashmap(self.constants, &self.mode, Some(&self.config));
        data.insert("manga_id".to_string(), id.to_string());

        if !metadata && page.is_none() {
            anyhow::bail!("You must set either metadata or page!");
        }

        if metadata {
            data.insert("metadata".to_string(), "1".to_string());
        } else if !metadata && page.is_some() {
            data.insert("page".to_string(), page.unwrap().to_string());
        }

        let resp = self
            .request::<MangaUrlResponse>(
                reqwest::Method::POST,
                "/manga/get_manga_url",
                Some(data),
                None,
            )
            .await?;

        if let Some(url) = resp.url {
            Ok(url)
        } else if let Some(url) = resp.metadata {
            Ok(url)
        } else {
            anyhow::bail!("No URL or metadata found")
        }
    }

    /// Get metadata for a chapter
    ///
    /// # Arguments
    /// * `id` - The chapter ID
    pub async fn get_chapter_metadata(&self, id: u32) -> anyhow::Result<MangaReadMetadataResponse> {
        let response = self.get_manga_url(id, true, None).await?;
        let url_parse = reqwest::Url::parse(&response)?;
        let host = url_parse.host_str().unwrap();

        let metadata_resp = self
            .inner
            .get(response)
            .header(
                reqwest::header::HOST,
                reqwest::header::HeaderValue::from_str(host).unwrap(),
            )
            .send()
            .await?;

        let metadata: MangaReadMetadataResponse =
            serde_json::from_str(&metadata_resp.text().await?)?;

        Ok(metadata)
    }

    /// Get the current user entitlements.
    ///
    /// This contains subscription information and other details.
    pub async fn get_entitlements(&self) -> anyhow::Result<AccountEntitlementsResponse> {
        let data = common_data_hashmap(self.constants, &self.mode, Some(&self.config));

        let response = self
            .request::<AccountEntitlementsResponse>(
                reqwest::Method::POST,
                "/manga/entitled",
                Some(data),
                None,
            )
            .await?;

        Ok(response)
    }

    /// Stream download the image from the given URL.
    ///
    /// The URL can be obtained from [`SJClient::get_manga_url`].
    ///
    /// # Parameters
    /// * `url` - The URL to download the image from.
    /// * `writer` - The writer to write the image to.
    pub async fn stream_download(
        &self,
        url: &str,
        mut writer: impl io::AsyncWrite + Unpin,
    ) -> anyhow::Result<()> {
        let url_parse = reqwest::Url::parse(url)?;
        let host = url_parse.host_str().unwrap();

        let res = self
            .inner
            .get(url)
            .header(
                reqwest::header::HOST,
                reqwest::header::HeaderValue::from_str(host).unwrap(),
            )
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!("Failed to download image: {}", res.status())
        }

        let mut stream = res.bytes_stream();
        while let Some(item) = stream.next().await {
            let item = item.unwrap();
            writer.write_all(&item).await?;
        }

        Ok(())
    }

    /// Perform a login request.
    ///
    /// Compared to other source crate, this method return the original response
    /// instead of the parsed config.
    ///
    /// # Arguments
    /// * `email` - The email of the user.
    /// * `password` - The password of the user.
    /// * `mode` - The mode to use for the login.
    pub async fn login(
        email: &str,
        password: &str,
        mode: SJMode,
    ) -> anyhow::Result<(AccountLoginResponse, String)> {
        let constants = crate::constants::get_constants(1);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(constants.ua),
        );
        headers.insert(
            reqwest::header::HOST,
            reqwest::header::HeaderValue::from_static(&API_HOST),
        );
        let referer = match mode {
            SJMode::VM => &constants.vm_name,
            SJMode::SJ => &constants.sj_name,
        };
        headers.insert(
            reqwest::header::REFERER,
            reqwest::header::HeaderValue::from_str(referer).unwrap(),
        );

        let x_header = format!("{} {}", constants.app_ver, *VALUE_PIECE);
        headers.insert(
            reqwest::header::HeaderName::from_static(&HEADER_PIECE),
            reqwest::header::HeaderValue::from_str(&x_header).unwrap(),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        let mut data = common_data_hashmap(constants, &mode, None);
        data.insert("login".to_string(), email.to_string());
        data.insert("pass".to_string(), password.to_string());

        let instance_id = data.get("instance_id").unwrap().clone();

        let response = client
            .post(format!("{}/manga/try_manga_login", BASE_API.as_str()))
            .form(&data)
            .header(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_str("application/x-www-form-urlencoded")
                    .unwrap(),
            )
            .send()
            .await?;

        let account_resp: AccountLoginResponse = parse_response(response).await?;

        Ok((account_resp, instance_id))
    }
}

fn common_data_hashmap(
    constants: &'static crate::constants::Constants,
    mode: &SJMode,
    config: Option<&SJConfig>,
) -> HashMap<String, String> {
    let mut data: HashMap<String, String> = HashMap::new();
    let app_id = match mode {
        SJMode::VM => VM_APP_ID,
        SJMode::SJ => SJ_APP_ID,
    };
    if let Some(config) = config {
        data.insert("trust_user_jwt".to_string(), config.token.clone());
        data.insert("user_id".to_string(), config.user_id.to_string());
        data.insert("instance_id".to_string(), config.instance.clone());
        data.insert("device_token".to_string(), config.instance.clone());
    } else {
        data.insert("instance_id".to_string(), generate_random_token());
    }
    data.insert("device_id".to_string(), constants.device_id.to_string());
    data.insert("version".to_string(), LIB_VERSION.to_string());
    data.insert(DATA_APP_ID.to_string(), app_id.to_string());
    data.insert(DATA_VERSION_CODE.to_string(), constants.app_ver.to_string());
    data
}

async fn parse_response<T>(response: reqwest::Response) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let stat_code = response.status();
    let headers = response.headers().clone();
    let url = response.url().clone();
    let raw_text = response.text().await.unwrap();
    let status_resp = serde_json::from_str::<SimpleResponse>(&raw_text.clone()).unwrap_or_else(|_| panic!(
        "Failed to parse response.\nURL: {}\nStatus code: {}\nHeaders: {:?}\nContents: {}\nBacktrace",
        url, stat_code, headers, raw_text
    ));

    if status_resp.is_err() {
        anyhow::bail!(
            "Response is not OK: {}",
            status_resp.error.unwrap_or("unknown error".to_string())
        )
    }

    let parsed = serde_json::from_str(&raw_text).unwrap_or_else(|err| {
        panic!(
            "Failed when deserializing response, error: {}\nURL: {}\nContents: {}",
            err, url, raw_text
        )
    });

    Ok(parsed)
}
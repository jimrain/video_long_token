//! Video Cache Warmer at the ege. This app works on HLS only. Once it reads a playlist manifest
//! it makes calls for the first X video segments so that they will be in cache.
use config::{Config, FileFormat};
use fastly::http::request::{PendingRequest, SendError};
use fastly::http::{header, Method, StatusCode, Url};
use fastly::{Error, Request, Response};
use fastly::Dictionary;
use lazy_static::lazy_static;
use m3u8_rs::playlist::{MasterPlaylist, Playlist, VariantStream};
use regex::Regex;
/// The name of a backend server associated with this service.
///
/// This backend is defined in my service but you can change it to whatever backend you want,
/// but it should have an m3u8 somewhere in it.
const BACKEND: &str = "ShastaRain_backend";


/// The entry point for your application.
///
/// This function is triggered when your service receives a client request. It could be used to
/// route based on the request properties (such as method or path), send the request to a backend,
/// make completely new requests, and/or generate synthetic responses.
///
/// If `main` returns an error, a 500 error response will be delivered to the client.
#[fastly::main]
fn main(mut req: Request) -> Result<Response, Error> {
    // Make sure we are running the version we think we are.
    println!("cpe-video-fastly-manifest-rewrite:{}", get_version());
    let dictionarySecrets = Dictionary::open("secrets");
    let xDemoCdn = dictionarySecrets.get("X-DEMO-CDN-TEST").unwrap_or("".to_string());
    // Filter request methods...
    if xDemoCdn == "" {
      return Ok(Response::from_status(StatusCode::INTERNAL_SERVER_ERROR).with_body_str("Service Configuration Invalid"));
    }
    if req.get_header_str("X-DEMO-CDN").unwrap_or("") != xDemoCdn {
      return Ok(Response::from_status(StatusCode::FORBIDDEN).with_body_str("Denied"));
    }
    let longToken = req.get_header_str("X-DEMO-LONG-TOKEN").unwrap_or("").to_owned();
    if longToken == "" {
      return Ok(Response::from_status(StatusCode::BAD_REQUEST).with_body_str("Missing Token"));
    }
    if req.get_method() != Method::GET {
        return Ok(Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
            .with_header(header::ALLOW, "GET, HEAD")
            .with_body_str("This method is not allowed\n"));
    }
    // JMR - change this back to "index.m3u8"
    if ! req.get_path().ends_with("m3u8") {
        return Ok(Response::from_status(StatusCode::BAD_REQUEST).with_body_str("Bad Request"));
    }

    // Acquire the manifest from origin, then decorate it with the value of longToken
    let xDemoCdnOrigin = dictionarySecrets.get("X-DEMO-CDN").unwrap_or("".to_string());
    req.set_header("X-DEMO-CDN", xDemoCdnOrigin);
    let mut resp = req.send(BACKEND)?;

    if resp.get_status() != StatusCode::OK {
      return Ok(resp);
    }
    let (_, mut manifest) = m3u8_rs::parse_master_playlist(resp.take_body_bytes().as_slice()).unwrap();
    // This will hold the list of variants after we add the long token.
    let mut modified_variants: Vec<VariantStream> = Vec::new();
    for mut variant in &manifest.variants {
        // Create a new variant to modify
        let mut new_variant = variant.clone();
        new_variant.uri = format!("{}/{}", longToken, variant.uri);
        println!("variant uri: {}", new_variant.uri);
        modified_variants.push(new_variant);
    }

    // Change the variants to the modified versions then convert it into a vector of u8 so we can
    // set the body in the response to the modifed version.
    manifest.variants = modified_variants;
    let mut v: Vec<u8> = Vec::new();
    manifest.write_to(&mut v).unwrap();
    resp.set_body_bytes(v.as_slice());

    Ok(resp)
    /*
    // If this is an m3u8 file parse it, other wise let it fall through to the backend.
    let path_str = req.get_path().to_owned();
    let req_url = req.get_url_str().to_owned();
    println!("URL: {}", req.get_url_str());
    let mut beresp = req.send(BACKEND)?;
    let mut new_resp = beresp.clone_with_body();
    // let mut body_bytes = new_resp.take_body_bytes();
    match m3u8_rs::parse_playlist_res(new_resp.take_body_bytes().as_slice()) {
        Ok(Playlist::MasterPlaylist(_pl)) => println!("Master playlist"),
        Ok(Playlist::MediaPlaylist(pl)) => {
            println!("Media Playlist. Path = {}", path_str);
            send_media_segments_requests_async(&pl, req_url)?;
        }
        Err(_e) => fastly::error::bail!("Invalid manifest"),
    }
    // I got what I needed so return the beresp in a Result
    Ok(beresp)
    */
}
/// This function reads the fastly.toml file and gets the deployed version. This is only run at
/// compile time. Since we bump the version number after building (during the deploy) we return
/// the version incremented by one so the version returned will match the deployed version.
/// NOTE: If the version is incremented by Tango this might be inaccurate.
fn get_version() -> i32 {
    Config::new()
        .merge(config::File::from_str(
            include_str!("../fastly.toml"), // assumes the existence of fastly.toml
            FileFormat::Toml,
        ))
        .unwrap()
        .get_str("version")
        .unwrap()
        .parse::<i32>()
        .unwrap_or(0)
        + 1
}
use async_compression::tokio::write::{
    BrotliDecoder, BrotliEncoder, DeflateEncoder, GzipDecoder, GzipEncoder, ZstdEncoder,
};
use async_compression::Level;
use filetime::{set_file_mtime, FileTime};
use lz4_flex::block::{compress_prepend_size, uncompressed_size};
use std::path::PathBuf;
use tokio::fs;
use tokio::fs::File;
use tokio::io::{copy, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tar::{Archive, Entry};

use super::error::Error;

async fn write_file(target: &PathBuf, data: &[u8]) -> Result<usize, Error> {
    let mut file = File::create(target).await?;
    file.write_all(data).await?;
    file.flush().await?;
    Ok(data.len())
}

async fn write_file_and_mtime(
    target: &PathBuf,
    data: &[u8],
    header: &tokio_tar::Header,
) -> Result<usize, Error> {
    if let Some(path) = target.parent() {
        fs::create_dir_all(path).await?;
    }
    let size = write_file(target, data).await?;
    if let Ok(mtime) = header.mtime() {
        set_file_mtime(target, FileTime::from_unix_time(mtime as i64, 0))?;
    }
    Ok(size)
}

async fn copy_mtime(file: &PathBuf, target: &PathBuf) -> Result<(), Error> {
    let f = File::open(file).await?;
    let meta = f.metadata().await?;
    set_file_mtime(target, FileTime::from_last_modification_time(&meta))?;
    Ok(())
}

async fn compress<'a, W>(file: &PathBuf, writer: &'a mut W) -> Result<u64, Error>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let mut r = File::open(file).await?;
    let size = copy(&mut r, writer).await?;
    Ok(size)
}

pub async fn gzip_encode(file: &PathBuf, target: &PathBuf, level: i32) -> Result<usize, Error> {
    let mut w = GzipEncoder::with_quality(Vec::new(), Level::Precise(level));
    compress(file, &mut w).await?;
    w.shutdown().await?;
    let size = write_file(target, &w.into_inner()).await?;
    copy_mtime(file, target).await?;
    Ok(size)
}

pub async fn gzip_decode(
    file: &mut Entry<Archive<File>>,
    target: &Option<PathBuf>,
) -> Result<Vec<u8>, Error> {
    let mut w = GzipDecoder::new(Vec::new());
    let _ = copy(file, &mut w).await?;
    w.shutdown().await?;
    let buf = w.into_inner();
    if let Some(target) = target {
        write_file_and_mtime(target, &buf, file.header()).await?;
    }
    Ok(buf)
}

pub async fn zstd_encode(file: &PathBuf, target: &PathBuf, level: i32) -> Result<usize, Error> {
    let mut w = ZstdEncoder::with_quality(Vec::new(), Level::Precise(level));
    compress(file, &mut w).await?;
    w.shutdown().await?;
    let size = write_file(target, &w.into_inner()).await?;
    copy_mtime(file, target).await?;
    Ok(size)
}

pub async fn zstd_decode(
    file: &mut Entry<Archive<File>>,
    target: &Option<PathBuf>,
) -> Result<Vec<u8>, Error> {
    let mut w = ZstdEncoder::new(Vec::new());
    let _ = copy(file, &mut w).await?;
    w.shutdown().await?;
    let buf = w.into_inner();
    if let Some(target) = target {
        write_file_and_mtime(target, &buf, file.header()).await?;
    }
    Ok(buf)
}

pub async fn brotli_encode(file: &PathBuf, target: &PathBuf, level: i32) -> Result<usize, Error> {
    let mut w = BrotliEncoder::with_quality(Vec::new(), Level::Precise(level));
    compress(file, &mut w).await?;
    w.shutdown().await?;
    let size = write_file(target, &w.into_inner()).await?;
    copy_mtime(file, target).await?;
    Ok(size)
}

pub async fn brotli_decode(
    file: &mut Entry<Archive<File>>,
    target: &Option<PathBuf>,
) -> Result<Vec<u8>, Error> {
    let mut w = BrotliDecoder::new(Vec::new());
    let _ = copy(file, &mut w).await?;
    w.shutdown().await?;
    let buf = w.into_inner();
    if let Some(target) = target {
        write_file_and_mtime(target, &buf, file.header()).await?;
    }
    Ok(buf)
}

pub async fn deflate_encode(file: &PathBuf, target: &PathBuf, level: i32) -> Result<usize, Error> {
    let mut w = DeflateEncoder::with_quality(Vec::new(), Level::Precise(level));
    compress(file, &mut w).await?;
    w.shutdown().await?;
    let size = write_file(target, &w.into_inner()).await?;
    copy_mtime(file, target).await?;
    Ok(size)
}

pub async fn deflate_decode(
    file: &mut Entry<Archive<File>>,
    target: &Option<PathBuf>,
) -> Result<Vec<u8>, Error> {
    let mut w = DeflateEncoder::new(Vec::new());
    let _ = copy(file, &mut w).await?;
    w.shutdown().await?;
    let buf = w.into_inner();
    if let Some(target) = target {
        write_file_and_mtime(target, &buf, file.header()).await?;
    }
    Ok(buf)
}

pub async fn snappy_encode(file: &PathBuf, target: &PathBuf) -> Result<usize, Error> {
    let buf = fs::read(file).await?;
    let mut w = snap::raw::Encoder::new();
    let size = write_file(target, &w.compress_vec(&buf)?).await?;
    copy_mtime(file, target).await?;
    Ok(size)
}

pub async fn snappy_decode(
    file: &mut Entry<Archive<File>>,
    target: &Option<PathBuf>,
) -> Result<Vec<u8>, Error> {
    let size = file.header().size()?;
    let mut w = snap::raw::Decoder::new();
    let mut buffer = Vec::with_capacity(size as usize);
    let mut handle = file.take(size);
    let _ = handle.read(&mut buffer).await?;
    let buf = w.decompress_vec(&buffer)?;
    if let Some(target) = target {
        write_file_and_mtime(target, &buf, file.header()).await?;
    }
    Ok(buf)
}

pub async fn lz4_encode(file: &PathBuf, target: &PathBuf) -> Result<usize, Error> {
    let buf = fs::read(file).await?;
    let size = write_file(target, &compress_prepend_size(&buf)).await?;
    copy_mtime(file, target).await?;
    Ok(size)
}

pub async fn lz4_decode(
    file: &mut Entry<Archive<File>>,
    target: &Option<PathBuf>,
) -> Result<Vec<u8>, Error> {
    let size = file.header().size()?;
    let mut buffer = Vec::with_capacity(size as usize);
    let mut handle = file.take(size);
    let _ = handle.read(&mut buffer).await?;
    let (_, buf) = uncompressed_size(&buffer)?;
    if let Some(target) = target {
        write_file_and_mtime(target, buf, file.header()).await?;
    }
    Ok(buf.to_owned())
}

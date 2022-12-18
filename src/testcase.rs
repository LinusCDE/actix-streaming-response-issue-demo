use anyhow::{bail, Result};
use lazy_static::lazy_static;
use pin_project_lite::pin_project;
use std::io::SeekFrom;
use std::io::{Error as IoError, Result as IoResult};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{self, Poll};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncSeekExt;
use tokio::io::{AsyncRead, AsyncSeek, BufReader};
use tokio::pin;
use tokio::sync::{RwLock, RwLockReadGuard};

lazy_static! {
    static ref TESTCASE_INSTANCE: RwLock<TestCaseController> = RwLock::new(TestCaseController {
        /// Using that is commented out for now. Would be some device file which returns data but doesn't have a
        /// valid path. Hence using actix-files would not work here.
        path: PathBuf::from("/dev/zero"),

        // Ensuring target cannot be written and read at the same time
        reading_refcounter: Arc::new(()),
        writing_refcounter: Arc::new(()),
    });
}

pub async fn controller() -> RwLockReadGuard<'static, TestCaseController> {
    TESTCASE_INSTANCE.read().await
}

/*
pub async fn controller_mut() -> RwLockWriteGuard<'static, TestCaseController> {
    TESTCASE_INSTANCE.write().await
}*/

pub enum TestCaseReader {
    Mock {
        inner: AsyncMockStream,
        counter: Arc<()>,
    },
    Real {
        inner: BufReader<File>,
        counter: Arc<()>,
    },
}

impl AsyncRead for TestCaseReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Mock { inner, .. } => {
                pin!(inner);
                inner.poll_read(cx, buf)
            }
            Self::Real { inner, .. } => {
                pin!(inner);
                inner.poll_read(cx, buf)
            }
        }
    }
}

pin_project! {
    pub struct AsyncMockStream {
        limit: u64,
        pos: u64,
    }
}

impl AsyncMockStream {
    pub fn new(limit: u64) -> Self {
        Self { limit, pos: 0 }
    }
}

impl AsyncRead for AsyncMockStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
        buf: &mut tokio::io::ReadBuf,
    ) -> Poll<IoResult<()>> {
        let size = ((self.limit - self.pos) as usize).min(buf.remaining());
        let mut new_data = vec![0u8; size];
        for i in 0..size {
            new_data[i] = ((self.pos + i as u64) % 256) as u8;
        }
        self.get_mut().pos += size as u64;
        buf.put_slice(&new_data);
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for AsyncMockStream {
    fn start_seek(self: Pin<&mut Self>, position: SeekFrom) -> IoResult<()> {
        let new_pos: i64 = match position {
            SeekFrom::Current(offset) => self.pos as i64 + offset,
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => self.limit as i64 + offset,
        };

        if new_pos < 0 || new_pos as u64 >= self.limit {
            return Err(IoError::from(std::io::ErrorKind::InvalidInput));
        }

        self.get_mut().pos = new_pos as u64;
        Ok(())
    }

    fn poll_complete(
        self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
    ) -> Poll<Result<u64, IoError>> {
        Poll::Ready(Ok(self.pos))
    }
}

pub struct TestCaseController {
    path: PathBuf,
    reading_refcounter: Arc<()>,
    writing_refcounter: Arc<()>,
}

impl TestCaseController {
    pub fn size(&self) -> u64 {
        1024 * 1024 * 512 // 0.5 GiB
    }

    pub fn is_writing(&self) -> bool {
        Arc::<()>::strong_count(&self.writing_refcounter) > 1
    }

    pub fn reading_count(&self) -> usize {
        Arc::<()>::strong_count(&self.reading_refcounter) - 1
    }

    /*pub fn is_reading(&self) -> bool {
        self.reading_count() > 0
    }*/

    pub fn is_mounted(&self) -> bool {
        return false;
    }

    pub async fn get_reader(&self, pos: Option<SeekFrom>) -> Result<TestCaseReader> {
        if self.is_writing() {
            bail!("Target is already being written to right now.")
        }
        if self.is_mounted() {
            bail!("Target has mounted partitions. Reading while content may change is not allowed.")
        }

        // For now just test this:
        if true {
            return Ok(TestCaseReader::Mock {
                inner: AsyncMockStream::new(self.size()),
                counter: self.reading_refcounter.clone(),
            });
        }

        // Would be something valid. If you wanna test this any large file should do as a stand-in
        let mut file = OpenOptions::new().read(true).open(&self.path).await?;
        if let Some(pos) = pos {
            file.seek(pos).await?;
        }
        Ok(TestCaseReader::Real {
            inner: BufReader::with_capacity(1024 * 512, file),
            counter: self.reading_refcounter.clone(),
        })
    }
}

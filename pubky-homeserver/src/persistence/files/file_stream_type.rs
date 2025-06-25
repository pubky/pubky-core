use bytes::Bytes;
use futures_util::Stream;

/// The type of the stream returned by the file service.
/// Box is needed to unify the types of the streams returned by the LMDB and OpenDAL services.
pub type FileStream = Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Unpin + Send>;

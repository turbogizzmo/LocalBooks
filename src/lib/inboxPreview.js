// Document Inbox previews: checking a file's bytes against the type its name gives it.

/** Whether `bytes` start with the signature of `mediaType` (PDF and the previewed images). */
export function contentMatches(mediaType, bytes) {
  const starts = (...sig) => sig.every((b, i) => bytes[i] === b);
  switch (mediaType) {
    case 'application/pdf':
      return starts(0x25, 0x50, 0x44, 0x46, 0x2d); // %PDF-
    case 'image/png':
      return starts(0x89, 0x50, 0x4e, 0x47);
    case 'image/jpeg':
      return starts(0xff, 0xd8, 0xff);
    case 'image/gif':
      return starts(0x47, 0x49, 0x46, 0x38);
    case 'image/webp':
      return starts(0x52, 0x49, 0x46, 0x46) && bytes[8] === 0x57 && bytes[9] === 0x45 && bytes[10] === 0x42 && bytes[11] === 0x50;
    default:
      return false;
  }
}

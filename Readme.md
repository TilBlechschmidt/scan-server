# ScanServer

Small utility to receive scans from MC363 multifunction printers and send them to a WebDAV endpoint.
Used to have internal storage but using WebDAV is way cooler :D

To operate, this needs three environment variables:

- `WEBDAV_URL`
- `WEBDAV_USER`
- `WEBDAV_PASS`

Note that you can append any directory name to the URL. This will then put the scanned files into a subdirectory of your server.

module github.com/nuetzliches/croniq/sdks/go

// No requirements: the published SDK is stdlib-only, and that is a property
// worth keeping. The one dependency this module used to carry —
// gopkg.in/yaml.v3, for parsing the shared conformance fixtures — now lives
// in the separate ./conformance module, because nothing a consumer can
// import ever reached it.
go 1.25

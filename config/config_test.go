package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestNormalizeDockerSQLiteDSNUsesMountedDataDir(t *testing.T) {
	root := t.TempDir()
	appDataDir := filepath.Join(root, "data")
	if err := os.MkdirAll(appDataDir, 0755); err != nil {
		t.Fatal(err)
	}
	Cfg = Config{StorageDriver: "sqlite", DatabaseDSN: "data/infinite-canvas.db?_pragma=busy_timeout(5000)"}

	normalizeDockerSQLiteDSN(appDataDir)

	want := filepath.Join(root, "data", "infinite-canvas.db") + "?_pragma=busy_timeout(5000)"
	if Cfg.DatabaseDSN != want {
		t.Fatalf("DatabaseDSN = %q, want %q", Cfg.DatabaseDSN, want)
	}
}

func TestNormalizeDockerSQLiteDSNLeavesLocalPathWithoutMountedDataDir(t *testing.T) {
	Cfg = Config{StorageDriver: "sqlite", DatabaseDSN: "data/infinite-canvas.db"}

	normalizeDockerSQLiteDSN(filepath.Join(t.TempDir(), "missing-data"))

	if Cfg.DatabaseDSN != "data/infinite-canvas.db" {
		t.Fatalf("DatabaseDSN = %q, want relative local path", Cfg.DatabaseDSN)
	}
}

func TestListenAddressPreservesDefaultNetworkBinding(t *testing.T) {
	Cfg = Config{Port: "8080"}

	if got := ListenAddress(); got != ":8080" {
		t.Fatalf("ListenAddress() = %q, want %q", got, ":8080")
	}
}

func TestListenAddressSupportsDesktopLoopbackBinding(t *testing.T) {
	Cfg = Config{BindHost: " 127.0.0.1 ", Port: "3101"}

	if got := ListenAddress(); got != "127.0.0.1:3101" {
		t.Fatalf("ListenAddress() = %q, want %q", got, "127.0.0.1:3101")
	}
}

package service

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/tigerowo/infinite-canvas/config"
	"github.com/tigerowo/infinite-canvas/model"
	"github.com/tigerowo/infinite-canvas/repository"
)

func TestPromptCatalogOnDemandLifecycle(t *testing.T) {
	config.Cfg.StorageDriver = "sqlite"
	config.Cfg.DatabaseDSN = filepath.Join(t.TempDir(), "prompts.db")
	db, err := repository.DB()
	if err != nil {
		t.Fatal(err)
	}
	var requests atomic.Int32
	var empty atomic.Bool
	var changed atomic.Bool
	const body = "This is the complete original prompt, not an index summary."
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if empty.Load() {
			_, _ = w.Write([]byte("no prompts here"))
			return
		}
		if changed.Load() {
			_, _ = w.Write([]byte("## Camera test\n```text\nA different prompt occupying the old position.\n```\n"))
			return
		}
		_, _ = w.Write([]byte("## Camera test\n```text\n" + body + "\n```\n"))
	}))
	defer server.Close()
	cat := model.PromptCategory{Category: "test-remote", Name: "Test remote", SourceType: "custom_url", PathOrURL: server.URL, Remote: true, Enabled: true}
	if err := repository.SavePromptCategory(cat); err != nil {
		t.Fatal(err)
	}
	legacy := model.Prompt{ID: "old-1", Title: "Camera test", Prompt: body, Category: cat.Category, Tags: []string{"视频创作", "运镜机位"}}
	if _, err := repository.SavePrompt(legacy); err != nil {
		t.Fatal(err)
	}
	local := model.Prompt{ID: "local-1", Title: "My local prompt", Prompt: "preserve my local content", Category: "system", Tags: []string{"local"}}
	if _, err := repository.SavePrompt(local); err != nil {
		t.Fatal(err)
	}
	if err := InitializePromptCatalog(); err != nil {
		t.Fatal(err)
	}
	if err := InitializePromptCatalog(); err != nil {
		t.Fatal(err)
	}
	if requests.Load() != 0 {
		t.Fatal("initialization unexpectedly used the network")
	}
	if db.Migrator().HasColumn(&model.PromptCatalog{}, "prompt") {
		t.Fatal("catalog must not have a full-text column")
	}
	q := model.Query{Category: cat.Category, PageSize: 20}
	list, err := ListPromptDirectory(q, false)
	if err != nil || list.Total != 1 {
		t.Fatalf("list: total=%d error=%v", list.Total, err)
	}
	id := list.Items[0].ID
	encoded, _ := json.Marshal(list)
	if strings.Contains(string(encoded), body) || list.Items[0].Prompt != "" {
		t.Fatal("list leaked full text")
	}
	if requests.Load() != 0 {
		t.Fatal("listing unexpectedly fetched remote content")
	}
	if _, err := PromptDetail("old-1"); err == nil {
		t.Fatal("legacy remote detail must not bypass on-demand loading")
	}
	detail, err := PromptDetail(id)
	if err != nil || detail.Prompt != body || requests.Load() != 1 {
		t.Fatalf("detail: %#v, %v", detail, err)
	}
	var count int64
	db.Model(&model.PromptFavorite{}).Count(&count)
	if count != 0 {
		t.Fatal("viewing created a favorite")
	}
	changed.Store(true)
	if _, err := PromptDetail(id); err == nil {
		t.Fatal("changed source was mistaken for the selected prompt")
	}
	changed.Store(false)
	bad := detail
	bad.Prompt = "tampered body"
	if err := FavoritePrompt(bad); err == nil {
		t.Fatal("mismatched content was accepted")
	}
	if err := FavoritePrompt(detail); err != nil {
		t.Fatal(err)
	}
	if err := FavoritePrompt(detail); err != nil {
		t.Fatal(err)
	}
	db.Model(&model.PromptFavorite{}).Count(&count)
	if count != 1 {
		t.Fatalf("favorite not idempotent: %d", count)
	}
	favs, err := ListPromptDirectory(model.Query{}, true)
	if err != nil || favs.Total != 1 || favs.Items[0].Prompt != "" || !favs.Items[0].Saved {
		t.Fatalf("favorites list: %#v, %v", favs, err)
	}
	if _, err := SyncPromptCategory(cat.Category); err != nil {
		t.Fatal(err)
	}
	list, err = ListPromptDirectory(q, false)
	if err != nil || list.Items[0].ID != id {
		t.Fatal("stable prompt identity changed during refresh")
	}
	empty.Store(true)
	if _, err := SyncPromptCategory(cat.Category); err == nil {
		t.Fatal("empty parse should fail closed")
	}
	list, _ = ListPromptDirectory(q, false)
	if list.Total != 1 {
		t.Fatal("failed refresh erased last good catalog")
	}
	server.Close()
	detail, err = PromptDetail(id)
	if err != nil || !detail.Saved || detail.Prompt != body {
		t.Fatalf("offline favorite: %#v, %v", detail, err)
	}
	if err := repository.DeletePromptFavorite(id); err != nil {
		t.Fatal(err)
	}
	if _, err := PromptDetail(id); err == nil {
		t.Fatal("unfavorited remote detail was still served from disk")
	}
	localDetail, err := PromptDetail(local.ID)
	if err != nil || localDetail.Prompt != local.Prompt {
		t.Fatal("local content was not preserved")
	}
	legacyRows, err := repository.LegacyPromptCategory(cat.Category)
	if err != nil || len(legacyRows) != 1 || legacyRows[0].Prompt != body {
		t.Fatal("legacy data was modified")
	}
	filtered, err := ListPromptDirectory(model.Query{Keyword: "Camera", Tags: []string{"视频创作"}}, false)
	if err != nil || filtered.Total != 1 {
		t.Fatalf("search/filter: %#v, %v", filtered, err)
	}
}

func TestPromptCatalogDoesNotPersistRawPreview(t *testing.T) {
	item := model.Prompt{Title: "Short", Prompt: "secret full text", Preview: "secret full text", Tags: []string{"tag"}}
	entries := catalogEntries(model.PromptCategory{Category: "test"}, []model.Prompt{item, item})
	data, _ := json.Marshal(entries)
	if len(entries) != 1 || strings.Contains(string(data), item.Prompt) {
		t.Fatal("dedupe or metadata-only contract failed")
	}
}

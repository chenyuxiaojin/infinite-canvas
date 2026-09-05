package service

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/tigerowo/infinite-canvas/model"
	"github.com/tigerowo/infinite-canvas/repository"
	"gorm.io/gorm"
)

type promptError string

func (e promptError) Error() string       { return string(e) }
func (e promptError) SafeMessage() string { return string(e) }

func promptContentHash(category string, item model.Prompt) string {
	return fmt.Sprintf("%x", sha256.Sum256([]byte(category+"\n"+item.Title+"\n"+strings.TrimSpace(item.Prompt))))
}

func catalogEntries(category model.PromptCategory, items []model.Prompt) []model.PromptCatalog {
	entries := []model.PromptCatalog{}
	seen := map[string]bool{}
	for _, item := range items {
		if strings.TrimSpace(item.Prompt) == "" || item.Title == "" {
			continue
		}
		hash := promptContentHash(category.Category, item)
		if seen[hash] {
			continue
		}
		seen[hash] = true
		source := category.GithubURL
		if source == "" {
			source = category.PathOrURL
		}
		// Preview is descriptive metadata only: never copy the prompt body here.
		entries = append(entries, model.PromptCatalog{ID: "catalog-" + hash, Title: item.Title, CoverURL: item.CoverURL, Tags: item.Tags, Category: category.Category, GithubURL: source, Preview: strings.Join(item.Tags, " · "), ContentHash: hash, CreatedAt: item.CreatedAt, UpdatedAt: item.UpdatedAt})
	}
	return entries
}

// Initialize metadata from the existing library once; never delete legacy data.
func InitializePromptCatalog() error {
	categories, err := repository.ListPromptCategories()
	if err != nil {
		return err
	}
	for _, category := range categories {
		if (!category.Remote && category.SourceType == "") || category.IndexUpdatedAt != "" {
			continue
		}
		items, err := repository.LegacyPromptCategory(category.Category)
		if err != nil {
			return err
		}
		if err := repository.ReplacePromptCatalog(category.Category, catalogEntries(category, items), time.Now().Format(time.RFC3339)); err != nil {
			return err
		}
	}
	return nil
}

func ListPromptDirectory(q model.Query, favorites bool) (model.PromptList, error) {
	if !favorites {
		if err := InitializePromptCatalog(); err != nil {
			return model.PromptList{}, err
		}
	}
	entries, tags, categories, total, err := repository.ListPromptDirectory(q, favorites)
	if err != nil {
		return model.PromptList{}, err
	}
	saved, err := repository.PromptFavoriteIDs()
	if err != nil {
		return model.PromptList{}, err
	}
	items := []model.Prompt{}
	for _, e := range entries {
		if e.Tags == nil {
			e.Tags = []string{}
		}
		items = append(items, model.Prompt{ID: e.ID, Title: e.Title, CoverURL: e.CoverURL, Tags: e.Tags, Category: e.Category, GithubURL: e.GithubURL, Preview: e.Preview, CreatedAt: e.CreatedAt, UpdatedAt: e.UpdatedAt, Remote: strings.HasPrefix(e.ID, "catalog-"), Saved: saved[e.ID]})
	}
	return model.PromptList{Items: items, Tags: tags, Categories: categories, Total: int(total)}, nil
}

func PromptDetail(id string) (model.Prompt, error) {
	favorite, err := repository.FindPromptFavorite(id)
	if err == nil {
		favorite.Prompt.GithubURL = favorite.SourceURL
		favorite.Prompt.Saved = true
		favorite.Prompt.Remote = strings.HasPrefix(id, "catalog-")
		return favorite.Prompt, nil
	}
	if !errors.Is(err, gorm.ErrRecordNotFound) {
		return model.Prompt{}, err
	}
	if !strings.HasPrefix(id, "catalog-") {
		item, err := repository.FindLocalPrompt(id)
		if item.Tags == nil {
			item.Tags = []string{}
		}
		return item, err
	}
	entry, err := repository.FindPromptCatalog(id)
	if err != nil {
		return model.Prompt{}, promptError("该目录条目已更新，请刷新目录后重新选择")
	}
	category, ok := repository.PromptCategoryByCode(entry.Category)
	if !ok || !category.Enabled {
		return model.Prompt{}, promptError("此订阅源当前不可用；已收藏的内容仍可离线使用")
	}
	// Source documents are parsed in memory, then discarded. No full-text disk cache.
	items, err := buildPromptCategoryItem(category)
	if err != nil {
		return model.Prompt{}, promptError("加载原文失败，请检查网络或来源；目录及收藏未受影响")
	}
	for _, item := range items {
		if promptContentHash(entry.Category, item) != entry.ContentHash {
			continue
		}
		item.ID, item.Category, item.GithubURL, item.Remote = entry.ID, entry.Category, entry.GithubURL, true
		return item, nil
	}
	return model.Prompt{}, promptError("来源内容已变更，请先更新目录后重新选择；不会使用旧序号替换成另一条提示词")
}

func FavoritePrompt(item model.Prompt) error {
	if len(item.Prompt) == 0 || len(item.Prompt) > 1024*1024 {
		return promptError("请先加载完整提示词再收藏")
	}
	if strings.HasPrefix(item.ID, "catalog-") {
		entry, err := repository.FindPromptCatalog(item.ID)
		if err != nil {
			return promptError("目录已更新，请重新选择后收藏")
		}
		if promptContentHash(entry.Category, item) != entry.ContentHash {
			return promptError("提示词与所选目录不一致，请重新加载")
		}
		item.Title, item.Category, item.GithubURL, item.CoverURL, item.Tags = entry.Title, entry.Category, entry.GithubURL, entry.CoverURL, entry.Tags
		item.Preview = entry.Preview
	} else {
		stored, err := repository.FindLocalPrompt(item.ID)
		if err != nil {
			return err
		}
		item = stored
		item.Preview = strings.Join(item.Tags, " · ")
	}
	return repository.SavePromptFavorite(model.PromptFavorite{Prompt: item, SourceURL: item.GithubURL, SavedAt: time.Now().Format(time.RFC3339)})
}

func UnfavoritePrompt(id string) error { return repository.DeletePromptFavorite(id) }

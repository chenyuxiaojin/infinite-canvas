package service

import (
	"log"
	"sync"

	"github.com/robfig/cron/v3"
	"github.com/tigerowo/infinite-canvas/model"
	"github.com/tigerowo/infinite-canvas/repository"
)

const defaultPromptSyncCron = "0 0 * * *"

var (
	promptSyncCron *cron.Cron
	promptSyncOnce sync.Once
	promptSyncMu   sync.Mutex
)

func StartPromptSyncScheduler() {
	promptSyncOnce.Do(func() {
		promptSyncCron = cron.New()
		promptSyncCron.Start()
	})
	RefreshPromptSyncScheduler()
}

func RefreshPromptSyncScheduler() {
	promptSyncMu.Lock()
	defer promptSyncMu.Unlock()
	if promptSyncCron == nil {
		return
	}
	for _, entry := range promptSyncCron.Entries() {
		promptSyncCron.Remove(entry.ID)
	}
	settings, err := repository.GetSettings()
	if err != nil {
		log.Printf("load prompt sync setting failed err=%v", err)
		return
	}
	setting := normalizePromptSyncSetting(settings.Private.PromptSync)
	if setting.Enabled == nil || !*setting.Enabled {
		return
	}
	if _, err := promptSyncCron.AddFunc(setting.Cron, SyncRemotePromptCategories); err != nil {
		log.Printf("add prompt sync cron failed cron=%s err=%v", setting.Cron, err)
	}
}

func SyncRemotePromptCategories() {
	SyncPromptSources()
}

type PromptSyncResult struct {
	Category string `json:"category"`
	Name     string `json:"name"`
	Error    string `json:"error,omitempty"`
}

func SyncPromptSources() []PromptSyncResult {
	results := []PromptSyncResult{}
	for _, category := range repository.PromptCategories() {
		if !category.Enabled || (!category.Remote && category.SourceType == "") {
			continue
		}
		result := PromptSyncResult{Category: category.Category, Name: category.Name}
		log.Printf("scheduled prompt sync start category=%s", category.Category)
		if _, err := SyncPromptCategory(category.Category); err != nil {
			log.Printf("scheduled prompt sync failed category=%s err=%v", category.Category, err)
			result.Error = "更新失败，已保留原目录"
		} else {
			log.Printf("scheduled prompt sync done category=%s", category.Category)
		}
		results = append(results, result)
	}
	return results
}

func normalizePromptSyncSetting(setting model.PromptSyncSetting) model.PromptSyncSetting {
	if setting.Cron == "" {
		setting.Cron = defaultPromptSyncCron
	}
	if setting.Enabled == nil {
		enabled := true
		setting.Enabled = &enabled
	}
	return setting
}

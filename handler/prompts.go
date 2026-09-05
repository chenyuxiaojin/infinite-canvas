package handler

import (
	"encoding/json"
	"net/http"

	"github.com/tigerowo/infinite-canvas/model"
	"github.com/tigerowo/infinite-canvas/service"
)

func Prompts(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Cache-Control", "no-store")
	result, err := service.ListPromptDirectory(parseQuery(r), r.URL.Query().Get("favorites") == "true")
	if err != nil {
		FailError(w, err)
		return
	}
	OK(w, result)
}

// PromptCategories 返回所有提示词分类/订阅源。
func PromptCategories(w http.ResponseWriter, r *http.Request) {
	OK(w, service.ListPromptCategories())
}

// SavePromptCategory 保存或更新自定义提示词分类/订阅源。
func SavePromptCategory(w http.ResponseWriter, r *http.Request) {
	var item model.PromptCategory
	if err := json.NewDecoder(r.Body).Decode(&item); err != nil {
		Fail(w, "解析请求体失败: "+err.Error())
		return
	}
	if item.Category == "" {
		Fail(w, "分类编码不能为空")
		return
	}
	if item.Name == "" {
		item.Name = item.Category
	}
	if err := service.SavePromptCategory(item); err != nil {
		FailError(w, err)
		return
	}
	OK(w, item)
}

// DeletePromptCategoryHandler 删除提示词分类及其内容。
func DeletePromptCategoryHandler(w http.ResponseWriter, r *http.Request, category string) {
	if category == "" {
		Fail(w, "分类编码不能为空")
		return
	}
	if err := service.DeletePromptCategory(category); err != nil {
		FailError(w, err)
		return
	}
	OK(w, true)
}

// SyncPrompts 即时触发指定分类或全量提示词源同步。
func SyncPrompts(w http.ResponseWriter, r *http.Request) {
	category := r.URL.Query().Get("category")
	if category != "" {
		result, err := service.SyncPromptCategory(category)
		if err != nil {
			FailError(w, err)
			return
		}
		OK(w, result)
		return
	}
	OK(w, service.SyncPromptSources())
}

func PromptDetail(w http.ResponseWriter, r *http.Request, id string) {
	w.Header().Set("Cache-Control", "no-store")
	item, err := service.PromptDetail(id)
	if err != nil {
		FailError(w, err)
		return
	}
	OK(w, item)
}

func FavoritePrompt(w http.ResponseWriter, r *http.Request) {
	var item model.Prompt
	if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 2*1024*1024)).Decode(&item); err != nil {
		Fail(w, "收藏内容无效或过大")
		return
	}
	if err := service.FavoritePrompt(item); err != nil {
		FailError(w, err)
		return
	}
	OK(w, true)
}

func UnfavoritePrompt(w http.ResponseWriter, r *http.Request, id string) {
	if err := service.UnfavoritePrompt(id); err != nil {
		FailError(w, err)
		return
	}
	OK(w, true)
}

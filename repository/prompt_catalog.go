package repository

import (
	"github.com/tigerowo/infinite-canvas/model"
	"gorm.io/gorm"
	"gorm.io/gorm/clause"
)

const catalogColumns = "id, title, cover_url, tags, category, github_url, preview, content_hash, created_at, updated_at"

func promptDirectory(db *gorm.DB, favorites bool) *gorm.DB {
	if favorites {
		return db.Table("prompt_favorites").Select("id, title, cover_url, tags, category, source_url AS github_url, preview, '' AS content_hash, created_at, updated_at")
	}
	// Legacy subscribed full text is preserved, but is not read by the new catalog.
	subscribed := db.Model(&model.PromptCategory{}).Select("category").Where("remote = ? OR source_type <> ?", true, "")
	local := db.Model(&model.Prompt{}).Select("id, title, cover_url, tags, category, '' AS github_url, '' AS preview, '' AS content_hash, created_at, updated_at").Where("category NOT IN (?)", subscribed)
	catalog := db.Model(&model.PromptCatalog{}).Select(catalogColumns).Where("category IN (?)", db.Model(&model.PromptCategory{}).Select("category").Where("enabled = ?", true))
	return db.Raw("? UNION ALL ?", catalog, local)
}

func ListPromptDirectory(q model.Query, favorites bool) ([]model.PromptCatalog, []string, []string, int64, error) {
	db, err := DB()
	if err != nil {
		return nil, nil, nil, 0, err
	}
	q.Normalize()
	base := func(tags bool) *gorm.DB {
		tx := db.Table("(?) AS directory", promptDirectory(db, favorites))
		if q.Keyword != "" {
			like := "%" + q.Keyword + "%"
			tx = tx.Where("title LIKE ? OR category LIKE ? OR preview LIKE ? OR tags LIKE ?", like, like, like, like)
		}
		if isActivePromptOption(q.Category) {
			tx = tx.Where("category = ?", q.Category)
		}
		if tags {
			tx = applyPromptTagsFilter(tx, q.Tags)
		}
		return tx
	}
	var total int64
	if err := base(true).Count(&total).Error; err != nil {
		return nil, nil, nil, 0, err
	}
	items := []model.PromptCatalog{}
	if err := base(true).Select(catalogColumns).Order("updated_at desc, id asc").Offset(q.Offset()).Limit(q.PageSize).Find(&items).Error; err != nil {
		return nil, nil, nil, 0, err
	}
	facets := []model.Prompt{}
	if err := base(false).Select("tags").Find(&facets).Error; err != nil {
		return nil, nil, nil, 0, err
	}
	categories := []string{}
	if err := db.Table("(?) AS directory", promptDirectory(db, favorites)).Distinct("category").Order("category asc").Pluck("category", &categories).Error; err != nil {
		return nil, nil, nil, 0, err
	}
	return items, promptTagsFromItems(facets), categories, total, nil
}

func LegacyPromptCategory(category string) ([]model.Prompt, error) {
	db, err := DB()
	if err != nil {
		return nil, err
	}
	items := []model.Prompt{}
	err = db.Where("category = ?", category).Find(&items).Error
	return items, err
}

func ReplacePromptCatalog(category string, items []model.PromptCatalog, updatedAt string) error {
	db, err := DB()
	if err != nil {
		return err
	}
	return db.Transaction(func(tx *gorm.DB) error {
		if err := tx.Where("category = ?", category).Delete(&model.PromptCatalog{}).Error; err != nil {
			return err
		}
		if len(items) > 0 {
			if err := tx.CreateInBatches(items, 100).Error; err != nil {
				return err
			}
		}
		return tx.Model(&model.PromptCategory{}).Where("category = ?", category).Update("index_updated_at", updatedAt).Error
	})
}

func FindPromptCatalog(id string) (model.PromptCatalog, error) {
	db, err := DB()
	item := model.PromptCatalog{}
	if err != nil {
		return item, err
	}
	err = db.Where("id = ?", id).First(&item).Error
	return item, err
}

func FindLocalPrompt(id string) (model.Prompt, error) {
	db, err := DB()
	item := model.Prompt{}
	if err != nil {
		return item, err
	}
	subscribed := db.Model(&model.PromptCategory{}).Select("category").Where("remote = ? OR source_type <> ?", true, "")
	err = db.Where("id = ? AND category NOT IN (?)", id, subscribed).First(&item).Error
	return item, err
}

func FindPromptFavorite(id string) (model.PromptFavorite, error) {
	db, err := DB()
	item := model.PromptFavorite{}
	if err != nil {
		return item, err
	}
	err = db.Where("id = ?", id).First(&item).Error
	return item, err
}

func PromptFavoriteIDs() (map[string]bool, error) {
	db, err := DB()
	if err != nil {
		return nil, err
	}
	ids := []string{}
	err = db.Model(&model.PromptFavorite{}).Pluck("id", &ids).Error
	saved := map[string]bool{}
	for _, id := range ids {
		saved[id] = true
	}
	return saved, err
}

func SavePromptFavorite(item model.PromptFavorite) error {
	db, err := DB()
	if err != nil {
		return err
	}
	return db.Clauses(clause.OnConflict{DoNothing: true}).Create(&item).Error
}

func DeletePromptFavorite(id string) error {
	db, err := DB()
	if err != nil {
		return err
	}
	return db.Where("id = ?", id).Delete(&model.PromptFavorite{}).Error
}

package model

// Prompt 提示词记录。
type Prompt struct {
	ID        string   `json:"id" gorm:"primaryKey"`
	Title     string   `json:"title"`
	CoverURL  string   `json:"coverUrl"`
	Prompt    string   `json:"prompt"`
	Tags      []string `json:"tags" gorm:"serializer:json"`
	Category  string   `json:"category" gorm:"index"`
	GithubURL string   `json:"githubUrl" gorm:"-"`
	Preview   string   `json:"preview"`
	CreatedAt string   `json:"createdAt"`
	UpdatedAt string   `json:"updatedAt"`
	Remote    bool     `json:"remote" gorm:"-"`
	Saved     bool     `json:"saved" gorm:"-"`
}

// PromptList 提示词分页结果。
type PromptList struct {
	Items      []Prompt `json:"items"`
	Tags       []string `json:"tags"`
	Categories []string `json:"categories"`
	Total      int      `json:"total"`
}

// PromptCategory 提示词分类。
type PromptCategory struct {
	Category       string `json:"category" gorm:"primaryKey"`
	Name           string `json:"name"`
	Description    string `json:"description"`
	GithubURL      string `json:"githubUrl"`
	SourceType     string `json:"sourceType"` // "remote_github", "local_markdown", "custom_url"
	PathOrURL      string `json:"pathOrUrl"`  // URL or absolute/relative file path
	Remote         bool   `json:"remote"`
	Enabled        bool   `json:"enabled" gorm:"default:true"`
	UpdatedAt      string `json:"updatedAt"`
	IndexUpdatedAt string `json:"indexUpdatedAt"`
}

// PromptCatalog deliberately has no prompt/full-text column.
type PromptCatalog struct {
	ID          string   `json:"id" gorm:"primaryKey"`
	Title       string   `json:"title"`
	CoverURL    string   `json:"coverUrl"`
	Tags        []string `json:"tags" gorm:"serializer:json"`
	Category    string   `json:"category" gorm:"index"`
	GithubURL   string   `json:"githubUrl"`
	Preview     string   `json:"preview"`
	ContentHash string   `json:"-"`
	CreatedAt   string   `json:"createdAt"`
	UpdatedAt   string   `json:"updatedAt"`
}

// Only an explicit favorite operation writes a new full-text copy.
type PromptFavorite struct {
	Prompt    `gorm:"embedded"`
	SourceURL string
	SavedAt   string
}

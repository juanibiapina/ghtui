package notificationssection

import (
	"testing"
	"time"

	tea "charm.land/bubbletea/v2"

	"github.com/dlvhdr/gh-dash/v4/internal/config"
	"github.com/dlvhdr/gh-dash/v4/internal/data"
	"github.com/dlvhdr/gh-dash/v4/internal/tui/components/notificationrow"
	"github.com/dlvhdr/gh-dash/v4/internal/tui/context"
	"github.com/dlvhdr/gh-dash/v4/internal/tui/theme"
)

// noopStartTask is a stub that returns nil for testing
func noopStartTask(task context.Task) tea.Cmd {
	return nil
}

func TestCheckoutPR(t *testing.T) {
	tests := []struct {
		name      string
		prNumber  int
		repoName  string
		repoPaths map[string]string
		wantErr   bool
		wantNil   bool
	}{
		{
			name:      "returns error when repo path not configured",
			prNumber:  123,
			repoName:  "owner/repo",
			repoPaths: map[string]string{},
			wantErr:   true,
			wantNil:   true,
		},
		{
			name:     "returns command when repo path is configured",
			prNumber: 123,
			repoName: "owner/repo",
			repoPaths: map[string]string{
				"owner/repo": "/path/to/repo",
			},
			wantErr: false,
			wantNil: false,
		},
		{
			name:     "returns command with tilde path",
			prNumber: 456,
			repoName: "my-org/my-repo",
			repoPaths: map[string]string{
				"my-org/my-repo": "~/projects/my-repo",
			},
			wantErr: false,
			wantNil: false,
		},
		{
			name:      "returns error for unconfigured repo even with other repos configured",
			prNumber:  789,
			repoName:  "other/repo",
			repoPaths: map[string]string{"owner/repo": "/path/to/repo"},
			wantErr:   true,
			wantNil:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			ctx := &context.ProgramContext{
				Config: &config.Config{
					RepoPaths: tt.repoPaths,
				},
				StartTask: noopStartTask,
			}

			cmd, err := CheckoutPR(ctx, tt.prNumber, tt.repoName)

			if tt.wantErr && err == nil {
				t.Errorf("CheckoutPR() error = nil, want error")
			}
			if !tt.wantErr && err != nil {
				t.Errorf("CheckoutPR() error = %v, want nil", err)
			}
			if tt.wantNil && cmd != nil {
				t.Errorf("CheckoutPR() returned non-nil cmd, want nil")
			}
			if !tt.wantNil && cmd == nil {
				t.Errorf("CheckoutPR() returned nil cmd, want non-nil")
			}
		})
	}
}

func TestCheckoutPRErrorMessage(t *testing.T) {
	ctx := &context.ProgramContext{
		Config: &config.Config{
			RepoPaths: map[string]string{},
		},
		StartTask: noopStartTask,
	}

	_, err := CheckoutPR(ctx, 123, "owner/repo")

	if err == nil {
		t.Fatal("CheckoutPR() expected error, got nil")
	}

	expectedMsg := "local path to repo not specified, set one in your config.yml under repoPaths"
	if err.Error() != expectedMsg {
		t.Errorf("CheckoutPR() error = %q, want %q", err.Error(), expectedMsg)
	}
}

func TestUpdateNotificationKeepsCursorOnNewLastItem(t *testing.T) {
	cfg, err := config.ParseConfig(config.Location{
		ConfigFlag:       "../../../config/testdata/test-config.yml",
		SkipGlobalConfig: true,
	})
	if err != nil {
		t.Fatalf("Failed to parse config: %v", err)
	}

	ctx := &context.ProgramContext{
		Config: &cfg,
	}
	ctx.Theme = theme.ParseTheme(ctx.Config)
	ctx.Styles = context.InitStyles(ctx.Theme)

	m := NewModel(0, ctx, config.NotificationsSectionConfig{}, time.Now())
	m.Notifications = []notificationrow.Data{
		{Notification: data.NotificationData{Id: "notif-A"}},
		{Notification: data.NotificationData{Id: "notif-B"}},
		{Notification: data.NotificationData{Id: "notif-C"}},
	}
	m.TotalCount = len(m.Notifications)
	m.Table.SetRows(m.BuildRows())

	m.LastItem()
	if got := m.CurrRow(); got != 2 {
		t.Fatalf("CurrRow() = %d, want 2 before removal", got)
	}

	m.Update(UpdateNotificationMsg{
		Id:        "notif-C",
		IsRemoved: true,
	})

	if got := m.CurrRow(); got != 1 {
		t.Fatalf("CurrRow() = %d, want 1 after removing the last notification", got)
	}

	current := m.GetCurrNotification()
	if current == nil {
		t.Fatal("GetCurrNotification() returned nil")
	}

	if got := current.GetId(); got != "notif-B" {
		t.Fatalf("GetCurrNotification().GetId() = %q, want %q", got, "notif-B")
	}
}

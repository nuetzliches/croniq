package croniqworker

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

type Config struct {
	BaseURL        string
	TenantID       string
	EnvironmentTag string
	ApiKey         string
	BearerToken    string
	HTTPClient     *http.Client
	Timeout        time.Duration
}
//go:build ignore

package croniqworker

// Deprecated: moved to sdk/runner-go.
	NextFireTimeUtc *time.Time `json:"nextFireTimeUtc,omitempty"`
	DeadLetterReason string    `json:"deadLetterReason,omitempty"`
}

type eventsRequest struct {
	EnvironmentTag string      `json:"environmentTag,omitempty"`
	RunnerId       string      `json:"runnerId"`
	Lease          Lease       `json:"lease"`
	Events         []WorkEvent `json:"events"`
}

// LanLink 房间码解析服务
//
// 极简 HTTP 服务，只做两件事：注册房间码、按码查询房间配置。
// 数据存单个 JSON 文件（默认 ./data/rooms.json），可直接备份。
//
// 用法：
//
//	go build -o lanlink-rooms .
//	./lanlink-rooms -addr 127.0.0.1:18080 -data ./data
//
// 环境变量：LANLINK_TOKEN（可选，开启后会校验 X-LanLink-Token 请求头）
package main

import (
	"crypto/rand"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"
)

// 去掉了容易看错的 0 / O / 1 / I
const alphabet = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"

const (
	maxRoomsPerFile = 10000
	maxBodyBytes    = 64 * 1024
	maxRelays       = 8
)

var preferredCode = regexp.MustCompile(`^[A-Z0-9]{3,16}-[A-Z0-9]{4}$`)

// Room 是一个房间码对应的连接配置。
type Room struct {
	Code          string   `json:"code"`
	NetworkName   string   `json:"network_name"`
	NetworkSecret string   `json:"network_secret"`
	Relays        []string `json:"relays"`
	IPMode        string   `json:"ip_mode"`
	Name          string   `json:"name,omitempty"`
	CreatedAt     int64    `json:"created_at"`
	ExpiresAt     int64    `json:"expires_at,omitempty"`
}

func (r Room) expired(now int64) bool {
	return r.ExpiresAt > 0 && now >= r.ExpiresAt
}

// Store 是带锁的 JSON 文件存储。
type Store struct {
	mu    sync.RWMutex
	path  string
	rooms map[string]Room
}

func NewStore(path string) (*Store, error) {
	s := &Store{path: path, rooms: map[string]Room{}}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return nil, err
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return s, nil
		}
		return nil, err
	}
	var rooms []Room
	if len(raw) > 0 {
		if err := json.Unmarshal(raw, &rooms); err != nil {
			return nil, fmt.Errorf("数据文件损坏：%w", err)
		}
	}
	for _, room := range rooms {
		s.rooms[room.Code] = room
	}
	return s, nil
}

func (s *Store) get(code string) (Room, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	room, ok := s.rooms[code]
	return room, ok
}

func (s *Store) create(room Room) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.rooms[room.Code]; ok {
		return errConflict
	}
	if len(s.rooms) >= maxRoomsPerFile {
		return fmt.Errorf("房间数已达上限 %d", maxRoomsPerFile)
	}
	s.rooms[room.Code] = room
	return s.flushLocked()
}

// flushLocked 原子落盘：先写临时文件再 rename，避免写一半断电。
func (s *Store) flushLocked() error {
	rooms := make([]Room, 0, len(s.rooms))
	for _, room := range s.rooms {
		rooms = append(rooms, room)
	}
	sort.Slice(rooms, func(i, j int) bool { return rooms[i].CreatedAt < rooms[j].CreatedAt })
	raw, err := json.MarshalIndent(rooms, "", "  ")
	if err != nil {
		return err
	}
	tmp := s.path + ".tmp"
	if err := os.WriteFile(tmp, raw, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, s.path)
}

func (s *Store) count() int {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return len(s.rooms)
}

var errConflict = fmt.Errorf("房间码已存在")

type server struct {
	store        *Store
	token        string
	ttl          time.Duration
	codeLen      int
	defaultRelay []string
}

func randomCode(n int) string {
	out := make([]byte, n)
	buf := make([]byte, n)
	if _, err := rand.Read(buf); err != nil {
		// 极端情况下的兜底，不影响功能
		now := time.Now().UnixNano()
		for i := range buf {
			buf[i] = byte(now >> (i * 5))
		}
	}
	for i := range out {
		out[i] = alphabet[int(buf[i])%len(alphabet)]
	}
	return string(out)
}

func (s *server) authorize(w http.ResponseWriter, r *http.Request) bool {
	if s.token == "" {
		return true
	}
	if r.Header.Get("X-LanLink-Token") == s.token {
		return true
	}
	writeJSON(w, http.StatusUnauthorized, map[string]string{"error": "token 不正确"})
	return false
}

func writeJSON(w http.ResponseWriter, status int, payload any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(payload)
}

type registerRequest struct {
	Code          string   `json:"code"`
	NetworkName   string   `json:"network_name"`
	NetworkSecret string   `json:"network_secret"`
	Relays        []string `json:"relays"`
	IPMode        string   `json:"ip_mode"`
	Name          string   `json:"name"`
	// 便于自测：秒级过期时间，0 表示用服务端默认 TTL
	ExpiresIn int64 `json:"expires_in"`
}

func (s *server) handleRegister(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeJSON(w, http.StatusMethodNotAllowed, map[string]string{"error": "只支持 POST"})
		return
	}
	if !s.authorize(w, r) {
		return
	}
	var req registerRequest
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, maxBodyBytes))
	if err := decoder.Decode(&req); err != nil {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "请求体不是合法 JSON"})
		return
	}

	req.NetworkName = strings.TrimSpace(req.NetworkName)
	req.NetworkSecret = strings.TrimSpace(req.NetworkSecret)
	if req.NetworkName == "" || req.NetworkSecret == "" {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "network_name / network_secret 不能为空"})
		return
	}
	relays := make([]string, 0, len(req.Relays))
	for _, relay := range req.Relays {
		relay = strings.TrimSpace(relay)
		if relay == "" || !hasRelayScheme(relay) {
			writeJSON(w, http.StatusBadRequest, map[string]string{"error": "中继地址需要以 tcp:// udp:// ws:// wss:// quic:// wg:// 开头"})
			return
		}
		relays = append(relays, relay)
	}
	if len(relays) == 0 {
		relays = append(relays, s.defaultRelay...)
	}
	if len(relays) == 0 {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "relays 不能为空"})
		return
	}
	if len(relays) > maxRelays {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": fmt.Sprintf("relays 最多 %d 条", maxRelays)})
		return
	}

	ipMode := strings.TrimSpace(req.IPMode)
	if ipMode == "" {
		ipMode = "dhcp"
	}

	now := time.Now().Unix()
	expiresAt := int64(0)
	if req.ExpiresIn > 0 {
		expiresAt = now + req.ExpiresIn
	} else if s.ttl > 0 {
		expiresAt = now + int64(s.ttl.Seconds())
	}

	room := Room{
		NetworkName:   req.NetworkName,
		NetworkSecret: req.NetworkSecret,
		Relays:        relays,
		IPMode:        ipMode,
		Name:          strings.TrimSpace(req.Name),
		CreatedAt:     now,
		ExpiresAt:     expiresAt,
	}

	code := strings.ToUpper(strings.TrimSpace(req.Code))
	if code != "" {
		if !preferredCode.MatchString(code) {
			writeJSON(w, http.StatusBadRequest, map[string]string{"error": "code 需形如 ROOM-8K3M"})
			return
		}
		room.Code = code
		if err := s.store.create(room); err != nil {
			if err == errConflict {
				writeJSON(w, http.StatusConflict, map[string]string{"error": "房间码已被占用"})
				return
			}
			writeJSON(w, http.StatusInternalServerError, map[string]string{"error": err.Error()})
			return
		}
	} else {
		created := false
		for attempt := 0; attempt < 8; attempt++ {
			room.Code = randomCode(s.codeLen)
			if err := s.store.create(room); err == nil {
				created = true
				break
			} else if err != errConflict {
				writeJSON(w, http.StatusInternalServerError, map[string]string{"error": err.Error()})
				return
			}
		}
		if !created {
			writeJSON(w, http.StatusInternalServerError, map[string]string{"error": "生成房间码失败，请重试"})
			return
		}
	}

	log.Printf("register %s network=%s relays=%d expires=%d", room.Code, room.NetworkName, len(room.Relays), room.ExpiresAt)
	writeJSON(w, http.StatusCreated, room)
}

func (s *server) handleResolve(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeJSON(w, http.StatusMethodNotAllowed, map[string]string{"error": "只支持 GET"})
		return
	}
	if !s.authorize(w, r) {
		return
	}
	code := strings.ToUpper(strings.TrimSpace(strings.TrimPrefix(r.URL.Path, "/r/")))
	if code == "" || strings.Contains(code, "/") {
		writeJSON(w, http.StatusBadRequest, map[string]string{"error": "缺少房间码"})
		return
	}
	room, ok := s.store.get(code)
	if !ok {
		writeJSON(w, http.StatusNotFound, map[string]string{"error": "房间码不存在"})
		return
	}
	if room.expired(time.Now().Unix()) {
		writeJSON(w, http.StatusGone, map[string]string{"error": "房间码已过期"})
		return
	}
	writeJSON(w, http.StatusOK, room)
}

func hasRelayScheme(relay string) bool {
	for _, scheme := range []string{"tcp://", "udp://", "ws://", "wss://", "quic://", "wg://"} {
		if strings.HasPrefix(strings.ToLower(relay), scheme) {
			return true
		}
	}
	return false
}

func (s *server) routes() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, http.StatusOK, map[string]any{"ok": true, "rooms": s.store.count()})
	})
	mux.HandleFunc("/r", s.handleRegister)
	mux.HandleFunc("/r/", s.handleResolve)
	return mux
}

func main() {
	addr := flag.String("addr", "127.0.0.1:18080", "监听地址，建议只绑 127.0.0.1 由 Nginx 反代")
	data := flag.String("data", "./data", "数据目录")
	token := flag.String("token", os.Getenv("LANLINK_TOKEN"), "可选访问 token，留空表示不校验")
	ttl := flag.Duration("ttl", 0, "房间码有效期，例如 720h；0 表示不过期")
	codeLen := flag.Int("codelen", 6, "自动生成的房间码长度（6-8）")
	relay := flag.String("relay", "", "请求未带 relays 时使用的默认中继，例如 tcp://1.2.3.4:11010")
	flag.Parse()

	if *codeLen < 6 || *codeLen > 8 {
		log.Fatalf("codelen 需要在 6-8 之间")
	}

	store, err := NewStore(filepath.Join(*data, "rooms.json"))
	if err != nil {
		log.Fatalf("加载数据失败：%v", err)
	}

	srv := &server{store: store, token: *token, ttl: *ttl, codeLen: *codeLen}
	if *relay != "" {
		if !hasRelayScheme(*relay) {
			log.Fatalf("relay 需要以 tcp:// 等协议开头")
		}
		srv.defaultRelay = []string{*relay}
	}

	httpServer := &http.Server{
		Addr:              *addr,
		Handler:           srv.routes(),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      10 * time.Second,
	}
	listener, err := net.Listen("tcp", *addr)
	if err != nil {
		log.Fatalf("监听 %s 失败：%v", *addr, err)
	}
	log.Printf("LanLink 房间码服务已启动 %s，数据 %s，token %s，ttl %s",
		*addr, store.path, maskToken(*token), *ttl)
	if err := httpServer.Serve(listener); err != nil {
		log.Fatal(err)
	}
}

func maskToken(token string) string {
	if token == "" {
		return "关闭"
	}
	return "已开启"
}

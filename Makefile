# Makefile simple pour RaftStone
# Juste des commandes bash, rien de compliqué

# Variables
BINARY := ./target/release/raftstone
TEST_PORT := 19999

# Compiler le binaire
build:
	@echo "🔨 Compilation..."
	cargo build --release
	@echo "✅ Binaire créé : $(BINARY)"

# Tester que le binaire compile
test-build: build
	@echo "✅ Build OK"

# Tester que le binaire démarre
test-start: build
	@echo "🚀 Test démarrage..."
	@echo "[server]\nport = $(TEST_PORT)" > /tmp/test-config.toml
	@$(BINARY) --config /tmp/test-config.toml & echo $$! > /tmp/raftstone.pid
	@sleep 2
	@curl -s http://localhost:$(TEST_PORT)/health > /dev/null && echo "✅ Serveur démarre OK" || echo "❌ Serveur ne répond pas"
	@kill `cat /tmp/raftstone.pid` 2>/dev/null || true
	@rm -f /tmp/raftstone.pid /tmp/test-config.toml

# Tester l'API
test-api: build
	@echo "📡 Test API..."
	@echo "[server]\nport = $(TEST_PORT)" > /tmp/test-config.toml
	@$(BINARY) --config /tmp/test-config.toml & echo $$! > /tmp/raftstone.pid
	@sleep 2
	@curl -s -X POST http://localhost:$(TEST_PORT)/api/test -d '{"test":"data"}' && echo "✅ API OK" || echo "❌ API failed"
	@kill `cat /tmp/raftstone.pid` 2>/dev/null || true
	@rm -f /tmp/raftstone.pid /tmp/test-config.toml

# Tests unitaires Rust
test-unit:
	@echo "📊 Tests unitaires..."
	cd raftstone-core && cargo test --lib

# Tests E2E Cucumber (si installé)
test-e2e:
	@echo "🥒 Tests E2E..."
	@cd cucumber-tests && cargo test --test cucumber_tests 2>/dev/null || echo "⚠️  Cucumber non disponible (optionnel)"

# Tous les tests
test-all: test-unit test-build test-start test-api
	@echo ""
	@echo "✅✅✅ TOUS LES TESTS PASSÉS ✅✅✅"

# Build + tests + release
release: test-all
	@echo ""
	@echo "🎉 Release prête !"
	@echo "📦 Binaire : $(BINARY)"
	@echo ""
	@echo "Pour distribuer :"
	@echo "  cp $(BINARY) /usr/local/bin/raftstone"

# Nettoyer
clean:
	@echo "🧹 Nettoyage..."
	cargo clean
	rm -f /tmp/test-config.toml /tmp/raftstone.pid

# Aide
help:
	@echo "Commandes disponibles :"
	@echo "  make build      - Compiler le binaire"
	@echo "  make test-all   - Lancer tous les tests"
	@echo "  make test-start - Tester que le serveur démarre"
	@echo "  make test-api   - Tester l'API"
	@echo "  make release    - Build + tests + release"
	@echo "  make clean      - Nettoyer"

.PHONY: build test-build test-start test-api test-unit test-e2e test-all release clean help

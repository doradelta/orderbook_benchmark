CXX = g++
CXXFLAGS = -std=c++20 -O3 -march=native -mtune=native -flto -fno-exceptions \
           -fno-rtti -Wall -Wextra -DNDEBUG -Isrc
LDFLAGS = -lpthread -flto

SRC_DIR = src
BUILD_DIR = build

.PHONY: build run benchmark clean

build: $(BUILD_DIR)/orderbook_system $(BUILD_DIR)/benchmark

$(BUILD_DIR)/orderbook_system: $(SRC_DIR)/main.cpp $(SRC_DIR)/*.h
	@mkdir -p $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) -o $@ $(SRC_DIR)/main.cpp $(LDFLAGS)

$(BUILD_DIR)/benchmark: $(SRC_DIR)/benchmark.cpp $(SRC_DIR)/*.h
	@mkdir -p $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) -o $@ $(SRC_DIR)/benchmark.cpp $(LDFLAGS)

CSV ?= ../btc_orderbook_updates.csv

run: build
	./$(BUILD_DIR)/orderbook_system $(CSV)

benchmark: build
	./$(BUILD_DIR)/benchmark $(CSV)

clean:
	rm -rf $(BUILD_DIR)

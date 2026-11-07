.PHONY: install docs media

# Install the ghtui binary to ~/.cargo/bin.
install:
	cargo install --path .

# Run the docs site at localhost:4321/ghtui/.
docs:
	cd docs && pnpm install && pnpm dev

# Re-record the docs videos and screenshots with vhs.
media:
	cargo build --release
	cd docs/src/assets && for tape in overview sections custom-commands screenshots; do PATH="$(CURDIR)/target/release:$$PATH" vhs $$tape.tape; done
	cd docs/src/assets && ffmpeg -y -loglevel error -ss 1 -i /tmp/ghtui-screenshots.mp4 -frames:v 1 tokyo.png
	cd docs/src/assets && ffmpeg -y -loglevel error -ss 4.5 -i /tmp/ghtui-screenshots.mp4 -frames:v 1 /tmp/ghtui-help.png
	cd docs/src/assets && ffmpeg -y -loglevel error -ss 6 -i /tmp/ghtui-screenshots.mp4 -frames:v 1 catpuccin.png
	cd docs/src/assets && ffmpeg -y -loglevel error -ss 8 -i /tmp/ghtui-screenshots.mp4 -frames:v 1 gruv.png
	cd docs/src/assets && magick /tmp/ghtui-help.png -crop 1600x440+0+386 +repage help.png
	cd docs/src/assets && magick tokyo.png -crop 920x32+0+0 +repage pr-sections.png

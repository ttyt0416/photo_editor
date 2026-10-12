# JPEG XL 합성 Brotli 메타데이터

`generate_jxl_compressed_metadata.py`가 Python Brotli 1.1.0으로 만든 자체 EXIF/XMP 기준 입력이다. 실제 사진이나 외부 메타데이터를 복사하지 않았다. EXIF는 두 TIFF 바이트 순서·방향8·Make와 시작 오프셋0/6, XMP는 한글 촬영자·원래 방향/치수·불투명 주석을 포함한다.

품질0/5/11과 window10/22/24의9조합·18압축 원문을 `manifest.json`의 SHA-256·복원 원문 SHA-256/크기로 확인한다. `over-budget.br`는 16MiB를 넘는 합성 바이트로 복원되며 앱의 보관 한도 거부를 검사하는 입력이다. 이미지 원본은 기존 `jxl`의 고정 libjxl 기준을 사용한다. 생성 JXL·PNG·프로젝트·디코더 출력은 `qa-output`에만 쓰고 검증 후 정리한다.

독립 확인은 `verify_jxl_compressed_metadata.py`로 실행한다. 압축 자산·앞/뒤 박스·원본/프로젝트와 PNG 방향/치수 갱신, 압축 데이터 불변을 검사하며 전체 JPEG XL 적합성 완료를 뜻하지 않는다.

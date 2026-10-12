# 합성 회색조 PSD/PSB 입력

`../generate_gray_psd_inputs.py`의 별도 struct/zlib 인코더로 만든 72개 입력이다. 8/16/32비트 × PSD/PSB × 무압축/RLE/ZIP/ZIP 예측 × 그룹·마스크/평면 알파/평면 불투명 조합이며 외부 파일의 아래→위 레코드 순서를 사용한다. 원래 수치와 SHA-256은 manifest.json에 있다. [Adobe 파일 규격](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)을 대조하고 설치된 psd-tools 1.10.9로 모든 회색/알파·레이어 채널을 독립 복원했다. Photoshop이 생성한 파일 또는 Photoshop 왕복 검증을 뜻하지 않는다.

32비트 값은 음수·HDR·signed zero를 포함한다. ICC는 원본 리소스 보존과 GRAY/RGB 헤더 불일치 처리를 검사하기 위한 합성 헤더이며 실제 색 변환 기준 프로파일이 아니다. 고정밀 마스크는 현재 8비트 편집 마스크로 변환된다. 추가 알파/별색·색 관리와 네이티브 회색조 출력은 별도 목표다.

`../verify_gray_psd_inputs.py`는 입력 해시/psd-tools와 Rust가 기록한 픽셀 배열 전체를 비교하고, 중성 RGB 출력 3종에서 원본 채널 값과 부적합 ICC 제외를 확인한다. 자체 프로젝트는 Rust 검사에서 원본 정밀도·리소스·레이어/마스크/클리핑과 signed zero를 포함한 float 비트를 왕복한다.

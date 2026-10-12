# JPEG XL 원본 ICC 기준

libjxl/conformance의 고정 커밋 `b1d0f990b03e57bf6d137c365cd5dc8b470b9191`에서 가져온 `bench_oriented_brg`와 `grayscale`이다. 소스 입력은 CC0이며 upstream은 기준 입력/복원 이미지의 테스트용 재현을 허용한다. 저장소 라이선스는 [기존 고지](../jxl/LICENSE)에 있다.

`fetch_jxl_icc_references.py`가 입력·설명과 설명 파일의 SHA-256에 해당하는 공개 객체를 받는다. `manifest.json`에 주소·원문/저장 해시·크기를 기록하며 큰 float 기준만 zlib로 저장한다. 프로파일 바이트는 수정하지 않는다. 회색조 입력의 `original.icc`와 화면용 `reference.icc`는 서로 다르므로 원본 보관 검사에서는 반드시 전자를 사용한다.

`verify_jxl_icc_projects.py`가 원본 ICC·픽셀 기준·프로젝트 CRC/버전·메타데이터 동시 보관을 독립 비교한다. 전체 ICC 변환이나 JPEG XL 적합성의 완료를 뜻하지 않는다. 생성 출력은 `qa-output/jxl-icc`에 쓰고 검증 후 정리한다.

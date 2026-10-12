# Windows 의존성 고지 자료

`windows-dependencies.json`은 현재 Cargo.lock의 Windows x86_64 MSVC 일반/빌드 의존성308개와 패키지 안의 원문528파일을 보존한다. dev 전용 의존성과 루트 앱은 제외한다. 일반 의존성이기도 한 패키지는 포함한다. 원래 이름/버전/라이선스 선언/저장소·소스 출처, 상대 파일 경로/문자 인코딩/원문 바이트 SHA-256/내용을 기록한다. vendored 패키지와 내장 글꼴 OFL/UFL·WebP COPYING 등 중첩된 원문도 포함한다.

`python scripts/collect_licenses.py`로 로컬 소스에서 오프라인 재생성한다. 이 자료는 실행 파일 안에 포함되므로 개발 도구/외부 파일 없이 도움말 → 오픈소스 라이선스에서 검색·선택·읽을 수 있다. 의존성을 변경하면 자료를 다시 생성하고 독립 검사로 실제 잠금 파일과 원문을 비교해야 한다.

현재18패키지는 배포된 crate에 원문 파일이 없어 선언만 기록했다. 파일이 있는 패키지도 해당 파일만으로 모든 구성 요소의 고지 요건을 충족한다고 판단하지 않는다. 앱 자체 라이선스 선택, 원문 없는 패키지의 정확한 저작권/허가 고지 확보, LGPL Rawler 소스/재링크 준비와 전체 배포 요건 검토, 실행 파일 패키징/서명/업데이트는 남은 목표다. `rawler/`, `file-content/`의 기존 자료도 유지한다.

The embedded working sRGB data asset and its local LittleCMS generation provenance are documented in `data/color/README.md`. The profile copyright field is "No copyright, use freely". This data asset is separate from the Cargo dependency notice inventory.

The neutral GRAY profile is derived locally from the same sRGB asset, with the original white point, tone curve and copyright preserved. Its generation and application-asset status are also documented in data/color/README.md.

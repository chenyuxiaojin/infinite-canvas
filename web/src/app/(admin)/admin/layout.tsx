import { redirect } from "next/navigation";

// 旧管理地址回到本地工作台，不挂载账号管理页面或发起其请求。
export default function AdminLayout() {
    redirect("/");
}
